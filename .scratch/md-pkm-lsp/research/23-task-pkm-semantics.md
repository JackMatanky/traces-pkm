# Ticket 23 research — Task & other PKM-specific semantics (consolidated)

Single consolidated research file for [ticket 23](../issues/23-task-and-pkm-semantics.md), assembled per the lead's instruction: three wave-1 subagents (codebase review via CodeGraph; crate research via rust-docs-mcp; concern decomposition), then five wave-2 research bundles (R1–R5) covering peer-LSP/obsidian precedent (local digests + web), Traces semantics archaeology, and leanest-path analysis. Every load-bearing claim carries a citation: `file:line` for the repo, `digest-name:line` for `docs/digests/`, URL for web. Research-only; no production code touched.

---

## Part 0 — Grounding corrections: the ticket's Question is factually wrong in five places

The Question text must be restated before its bullets are answerable. Verified against current source:

1. **`ListItem` is not at `src/note/lists.rs:64`, has no `children`, and no `position: ListItemPosition`.** Actual shape: `src/note/lists.rs:39-49` — `text`, `kind`, `fields`, `tags`, `line: SourceLine`, `parent: Option<SourceLine>`, `depth: u8`, `is_ordered`. `ListItemPosition` was dissolved in commit `50bdc493` (2026-09-18, "flatten `Note.lists` and drop `LISTS` table"); hierarchy is now `depth` + `parent` source line (`lists.rs:3-6`). The *relationship* the ticket names survives as the `parent` field.
2. **`kind` is `ListItemType::Task(TaskListItem)`, not `Task(TaskStatus)`** (`lists.rs:233`, `lists.rs:347`); `TaskStatus` is a component of `TaskListItem`.
3. **The diagnostics parenthetical is wrong on both halves.** An unknown checkbox symbol is *not* "silently non-task": `TaskStatusMap::resolve` falls back to a synthetic **Todo** for any unconfigured symbol (`src/task.rs:140-152`; doc `task.rs:138-139` "preserving `symbol` on the fallback **for diagnostics**"; test `src/note/parser/list.rs:696-715` "unknown marker must never be downgraded"). And `TaskStatusType::NonTask` is a *configured status kind* (`src/config/raw.rs:153`, `model.rs:713`) that classification never consults — its only runtime effects are `completed() -> Some(false)` (`task.rs:307`) and the display string `"non-task"` (`src/query/results.rs:379`).
4. **Task vs Checkbox is a tag-filter decision, not a status decision** (`list.rs:185-201`): marked + (no filters or tag match) → `Task`; marked + filter present but no match → `Checkbox`; unmarked → `Plain`. Any design assuming "known status ⇒ task" is wrong once `tag_filters` is configured (`tests/integration/task_tag_filters.rs`).
5. **Ticket 18's alternative is moot.** Ticket 18 is resolved and its Answer + research contain zero mentions of task lines/emoji (grep-verified); its scope was date-*filename* matching, `[periodic]` config, NL date parsing for links, and link diagnostics. "Fall entirely under 18" would require reopening 18. What 18 *does* provide for reuse: the ±7-day candidate generator, the NL date parser, `DateValue` (see Part VIII).

Also worth noting (not in the Question but corrected during research): "last-wins per date slot" is wrong — it's **first-wins** (`TaskDateSet::insert` doc `src/task.rs:479-494`; guard `src/note/parser/task.rs:49-56`; tests `task.rs:234-245`, `list.rs:1386`). Emoji beats inline field per slot; `done` > `completion` alias.

---

## Part I — Codebase baseline (wave-1 codegraph review)

### I.1 Parsing pipeline

- `parse_markdown(input) -> Note` is **infallible** — no `Result`, no diagnostics channel (`src/note/parser.rs:105-118`). Config enters only via `MarkdownParserInput`: `input.tasks().statuses()` + `tag_filters()` → `ParserContext` (`parser.rs:108-109`).
- `MARKDOWN_OPTIONS = ENABLE_YAML_STYLE_METADATA_BLOCKS | ENABLE_WIKILINKS` — **`ENABLE_TASKLISTS` deliberately off** (`parser.rs:74-75`); pulldown-cmark's marker scanner only knows `[ ]`/`[x]`, so Traces' own scanner is the single source of truth (confirmed against pulldown-cmark source, Part II).
- Marker grammar (`src/note/parser/marker.rs:47-66`): `[` + one non-`]` char + **one ASCII whitespace** (EOL counts). `[]`, `[xx]`, `[x]x`, NBSP-after-marker all rejected → plain text (`marker.rs:411-424`, `454-458`). `MarkerAccumulator` buffers ≤8 leading bytes; `scan_marker_prefix` returns `MarkerPrefix::{Continue, Reject, Done}` (`marker.rs:66`), with `Incomplete` states (`[`, `[c`, `[c]` awaiting ws) — exactly the typing-in-progress states completion should fire on.
- Classification at `end_item` (`list.rs:166-204`): marker → `statuses.resolve(symbol)` → Task/Checkbox by tag filters → else Plain.
- One `TaskScan` tokenization pass per item feeds priority, dates, clean text (`list.rs:177-181`). `clean_text` strips in prescribed order: tag filters → date shorthand → priority emoji → inline fields (test `strips_in_prescribed_order`, `list.rs:1560`).

### I.2 AST shape

`ListItem` (`lists.rs:39-49`); `ListItemType::{Plain, Checkbox(unit — symbol discarded!), Task(TaskListItem)}` (`lists.rs:233-240`); `TaskListItem { dates, priority, status, fully_complete }` (`lists.rs:347`). Flat document order; hierarchy via `depth` + `parent: SourceLine`. `Note::tasks()` filters `kind().is_task()` (`src/note/model.rs:178`); `descendants_of` walks depth+contiguity (`lists.rs:643-648`). `Checkbox` is a unit variant and marker bytes are stripped from `ListText.raw` for recognized items (`lists.rs:495-501`) — the symbol is unrecoverable post-parse for filtered items.

**Span reality**: the full item byte span is computed at flush then *discarded* — `let _span = item_frame.start.close(end);` (`list.rs:174`). `ListItem` keeps line/parent/depth only. `ListText.raw` is built from pulldown text events with marker stripped and inline markup absent → raw offsets drift from source offsets when markup is present (`lists.rs:516-560`, `list.rs:432-434`) — critical for Part VIII's re-lex recommendation.

### I.3 Config `[tasks]`

- Raw schema: **only** `tag_filters` + `statuses`, `deny_unknown_fields` (`src/config/raw.rs:107-124`). `RawTaskStatus { symbol, name, kind }`, kinds `todo|in-progress|on-hold|done|cancelled|non-task` (`raw.rs:138-153`).
- Merge: local-non-empty > global > default (`src/config/builder.rs:137-155`); `TaskConfig::try_from` (`src/config/model.rs:666-701`).
- Defaults (`src/task.rs:378-415`): `' '` Todo, `x` Done, `X` Done, `/` In Progress, `-` Cancelled, `!` On Hold. Prohibited symbols `()[]{} ` rejected at `TaskStatusMap::insert` (`task.rs:199-215`).
- Consumption chain: `Config::tasks()` → cloned into `IndexerService` (`src/index/service.rs:35-47`) → `MarkdownParserInput` per file (`service.rs:271-283`) → `ParserContext` → `end_item`. **Classification is baked into `Note.lists` at parse time and persisted in redb** (spec ADR-0005, `.scratch/task-system/spec.md:29-34`).
- Visibility: `TaskConfig::statuses()` is `pub(crate)` (`model.rs:609`); `tag_filters()` is `pub` (`model.rs:619`).

### I.4 Emoji date/priority shorthand

- Tables: `TaskDateType::EMOJIS` (`task.rs:767-776`) — ➕ Created, ⏳ Scheduled, 🛫 Start, 📅 and 🗓 Due, ✅ Done, ❌ Cancelled; `TaskPriority::EMOJIS` (`task.rs:962-968`) — 🔺⏫🔼🔽⏬. VS16 (`U+FE0F`) variants are paired `#[token]`s in the lexer (`lexer.rs:109-141`); `from_emoji` strips trailing VS16 (`task.rs:794-810`).
- Zero configurability (const slices; `[tasks]` accepts only statuses/tag_filters).
- **Three nested scopes** (Part VIII.3): S1 clean-text stripping runs on *every* list item; S2 field conversion (`TaskFieldEmojis::Include`) runs iff `marker.is_marked()` (`list.rs:94-99`); S3 `TaskListItem` extraction additionally requires tag-filter match.
- Invalid dates are silently skipped: `task_date_callback` returns `Filter::Skip` on ISO-shape parse failure (`lexer.rs:217-248`) — glyph survives in `clean_text`, no token, no span.
- Doc gap: `src/note/mod.rs:13-16` lists only `🗓️ ➕ 🛫 ⏳ ✅`, omitting 📅, ❌ and all priority emojis. Hover copy must cite `task.rs` tables.

### I.5 Query & consumers

`QueryMode::Tasks` (`src/query/builder.rs:21-29`) → `QueryService::tasks` (`service.rs:249-255`). Task fields `list.status/status_type/status_symbol/completed/priority/due/done/created/start/scheduled/cancelled/fully_complete` (`src/query/grammar/field.rs:215-254`); obsolete `task.*` namespace rejected with did-you-mean → `list.*` (`field.rs:61-106`). Consumers: CLI `traces task` (`src/cli/task.rs`), template `tasks` helper (`src/template/engine/query.rs:146-171`).

### I.6 Diagnostics/symbols status quo

- Diagnostics today: miette for query syntax only (`src/query/error.rs`); config errors are `thiserror` (`src/config/error.rs`); `tracing::warn!` elsewhere. **No note-level lint exists — rumdl owns Markdown linting.** ADR-0004 says domain modules must not own miette, but `src/query/error.rs` has done so since commit `fc9fb2e3` (2026-08-14, after the ADR) — query is the standing precedent and the ADR text is stale.
- Symbols: **no symbol/outline code anywhere**; headings not retained in the AST as of today's code (ticket 11's Answer decided a flat `Heading` node *should* exist — design against the decision, not the code).
- No `src/lsp/`, no tower-lsp/lsp-types/ropey/tokio in Cargo.toml (ticket 09 decided tower-lsp-server; not yet wired).

### I.7 Glossary (authoritative)

`Task` / `Task Status` at `src/note/CONTEXT.md:50-59`; `[tasks]` at `src/config/CONTEXT.md:60-70`; `Query Mode` at `src/query/CONTEXT.md:25-28`; flat list model at `src/note/lists.rs:3-6`; domain spec `.scratch/task-system/spec.md` (318 lines, authoritative for task-system semantics).

---

## Part II — Crate & dependency verdict (wave-1 rust-docs research)

**Verdict: zero new dependencies for ticket 23.**

| Candidate | Status | Finding |
|---|---|---|
| pulldown-cmark 0.13.4 | in tree, cached | `Event::TaskListMarker(bool)` only with `ENABLE_TASKLISTS`, hardcoded ASCII `[ ]`/`[x]`, no symbol — API *requires* Traces' custom scanner for custom symbols. `into_offset_iter()` gives UTF-8 byte ranges (already used, `parser.rs:113`). |
| logos 0.16.1 | in tree, cached | Already proven for emoji/VS16/field tokens (`lexer.rs:106-149`); char-boundary-safe; compile-time conflict errors (`GraphError::Disambiguation`/`EmptyMatch`). Marker scanning must stay position-aware (`scan_marker_prefix`), not lexed — body `[x]` must not be a marker. |
| regex 1.13.1 | in tree | Keep out of marker matching (position-dependent structural rule). |
| chrono 0.4.45 | in tree | The date parser for shorthand values. `jiff` cached but not a dep — not needed. |
| `emojis` / `unicode-segmentation` | not needed | Plain `str` matching suffices; VS16 handled by paired tokens. |
| tower-lsp-server 0.23 / lsp-types 0.97 | cached, decided by ticket 09 | `CompletionItem.text_edit` (single-line, must contain request position); `insert_text` explicitly discouraged by lsp-types docs in favor of `textEdit`; `filter_text`/`sort_text` fall back to label. |

Ecosystem axis: `tower-lsp-server` as de-facto Rust LSP standard is already web-verified in `research/07-rust-lsp-framework-crates.md`. For logos/emoji-crate ecosystem consensus: not assessable from local sources and moot — both are already in-tree (or unnecessary).

---

## Part III — Concern decomposition (wave-1) → bundles

19 concerns (C1–C19) across the ticket's four bullets, grouped into five research bundles:

| Bundle | Concerns | Scope |
|---|---|---|
| **R1** | C4 precedent, C2 offer shape, C1 predicate/leanest | checkbox completion (bullet a) |
| **R2** | C6 peer diagnostics, C7 rumdl overlap, C5 partial detectability | diagnostics (bullet b) |
| **R3** | C5 full state inventory, C9 NonTask, C18 config lifetime, C19 shared predicates, C8 config-vs-document boundary | Traces semantics |
| **R4** | C11 precedent, C12 nesting/spec, C13 ranges, C14 workspace cost, C10 ownership | symbols (bullet c) |
| **R5** | C15 emoji completion precedent, C16 token scope/seams, C17 hover | emoji markers (bullet d) |

Ticket-wording ambiguities the grilling must resolve regardless of research: **A1** status *cycling/toggling* not covered by any bullet (→ 27 code actions?); **A2** diagnostics parenthetical wrong (Part 0); **A3** "document symbols / a task outline" conflates documentSymbol with workspace/symbol; **A4** 18-alternative moot; **A5** task hover otherwise unmentioned; **A6** tag-filter classification load-bearing but absent from the Decide list; **A7** no graph edges to 24/25/27 despite supplying their inputs; **A8** NonTask resolution standing.

C10 carries no research (pure ownership decision).

---

## Part IV — R1: Checkbox-status completion (bullets a)

### IV.1 Precedent (C4): ABSENT everywhere

| LSP | Task/checkbox completion? | Evidence |
|---|---|---|
| Marksman | No (wikiDoc/heading/reference/tag only) | `lsp_artempyanykh-marksman-digest.txt:11150-11157`, `:4751`, `:5189-5235` |
| Markdown Oxide | No (callout/footnote/link/tag) | `lsp_feel-ix-343-markdown-oxide-digest.txt:2028-2039`, `-src-digest.txt:4867` |
| Microsoft markdown-ls | No — explicitly "we do not offer extensive support for task lists" | `lsp_microsoft-vscode-markdown-languageservice-digest.txt:908`, `:145-151` |
| rumdl | No (fence-language + link target only) | `lsp_rvben-rumdl-src-digest.txt:29374-29383` |
| zk | No (links/tags) | `zk-digest.txt:9769`, `zk-src-digest.txt:3080`, `:4344-4358` |

Ecosystem: status changes are **commands/cycles/menus**, never inline symbol completion — Obsidian core toggle (Cmd+L), vscode-md-checkbox Alt+C, markdown-all-in-one Alt+C, checkbox.nvim/checkmate.nvim, obsidian-task-status quick-pick. Closest near-precedent: bfm.nvim blink.cmp source (triggers `@`,`/`; still cycles status). Adjacent family: **task metadata completion** (obsidian-tasks Auto-Suggest — never touches the symbol). **Traces would be first with inline symbol completion — first-mover status is itself a grilling question.**

### IV.2 Offer shape (C2)

obsidian-tasks design facts: gating on recognized task line with cursor in the *description* (`canSuggestForLine`, `obsidian_obsidian-tasks-src-digest.txt:23973-23986`; docs `:2223-2240` — Traces inverts this to cursor-*on*-checkbox); never rewrites the symbol (`addTaskPropertySuggestions`, src `:23384`); label precedent `'Tasks: Change status to: [ ] Todo'` → `[symbol] Name` (docs `:3122-3218`); over-eager-popup bug #1509 (bracket regex fired anywhere) ⇒ **gate structurally (bullet + `[`), never bare `[`**; knobs `autoSuggestMinMatch: 0`, `MaxItems: 20`.

LSP mechanics (lsp-types 0.97, `src/completion.rs:419-542`): use `text_edit` (never `insert_text` — client reinterpretation); range single-line + contains position; `filter_text` falls back to label.

Proposed shape: one item per configured status minus `NonTask`; `label` = `[x] Done`; `text_edit` replaces **inner span only** (char between brackets), leaving brackets/whitespace; `filter_text` = bare symbol (Todo's `' '` symbol needs a special case); explicit `sort_text` (map is a HashMap → nondeterministic) + consider `preselect`; defaults offered `' x X / - !`. Incomplete-marker case (`- [` with no `]`): insert `x] ` or defer to v1-only-when-complete (grilling call). `[` trigger is claimed by wikilinks (15) and inline fields (19) → seam to ticket 24.

### IV.3 Leanest path (C1)

Predicate is **buffer-local O(line)**: indent + bullet + ws + `[`, cursor between brackets; no AST, no reparse, no index. Reuse `scan_marker_prefix` (Complete/Incomplete/Rejected) — currently `pub(super)`, needs widening or colocation (packaging call for 34). **Gap: no status iterator** — `TaskStatusMap` exposes only `by_symbol`/`resolve`/`by_name`/`by_type`/`insert` (`task.rs:127-198`); need `pub(crate) fn iter()` over the insertion-ordered `kinds` Vec. Cost: statuses resolved once at config load; completion reads the map at request time → **zero didChange/keystroke cost**, ≤6 items, no new crates. Seams to 24: `[` trigger routing; `CodeRegion`-in-fenced-code suppression (19's precedent).

---

## Part V — R2: Diagnostics landscape & rumdl overlap (bullet b)

### V.1 Peer behavior (C6): zero per-note task diagnostics anywhere

- **LSP peers**: none — Marksman 0 task/checkbox hits (all "task" hits are `System.Threading.Tasks`); Markdown Oxide only prose; Microsoft only link/symbol/folding contexts; zk none.
- **Closest analog, obsidian-tasks**: unknown symbol → synthesized `Unknown` status, type `TODO`, editor path **silent** (`obsidian_obsidian-tasks-src-digest.txt:21994-22005`; docs "Unknown Statuses" `:4948-4960`). `NON_TASK` is a configured Status Type, not a classification filter (`docs-digest:6555-6570`). Only surfacing: **config-scoped on-demand report** — `StatusSettingsReport.ts` problems, "Review and check your Statuses" button (src `:22700-22775`, docs `:5766-5800`), plus query escape hatches ("Finding unread emojis" docs `:2464`, "Finding Tasks with Invalid Dates" `:3507`).
- **Invalid dates: deliberate documented silence** — "Tasks does not automatically report any problem tasks that have invalid dates... silently not be found by date-based searches" (docs `:3505`).
- **Linters**: markdownlint has no checkbox-validity rule (MD052 *suppresses* checkbox false-positives, `ignored_labels` defaults `["x"]`); markdownlint-rs MD011 skips GFM checkboxes; remark-lint has only *style* rules on recognized GFM checkboxes (`remark-lint-checkbox-character-style`, `-content-indent`) — `- [?]` is never inspected. The ecosystem's recurring checkbox noise is the *opposite* error: over-eager link/reference rules firing on checkboxes (markdownlint #1524, eslint/markdown #335).

### V.2 rumdl overlap (C7): structurally none on the core diagnostic

- 83 rules, none inspects checkbox symbol validity (`docs/rules.md`, `lsp_rvben-rumdl-docs-digest.txt:17387-17440`; 0 hits for status-symbol greps).
- rumdl parses task lists **flavor-blind, GFM-only**: `options.insert(Options::ENABLE_TASKLISTS); let _ = flavor;` (`lsp_rvben-rumdl-src-digest.txt:27031-27036`) → `- [z]` is an ordinary list item to rumdl.
- Same-line adjacency only (different message, spacing/format rules, all default-on): MD064 (space runs; checkbox-exempt; obsidian flavor recognizes `[/]`,`[-]`,`[>]` as exemptions only — `docs-digest:11722,15943-15949`), MD030 (marker spacing, explicitly not checkbox spacing `:5842-5900`), MD077/MD076 (accommodate task bodies `:13620,13802-13805`). MD054 has an explicit test `test_task_list_checkboxes_not_flagged_as_shortcut` (`lsp_rvben-rumdl-digest.txt:235406`).
- **Never emit `MD…` codes** — that namespace is rumdl's (`docs-digest:1883-2210`).

### V.3 Detectability (C5-partial, R2's pass)

| State | Detectable post-parse? | Cost | Peer precedent | rumdl overlap |
|---|---|---|---|---|
| (i) unconfigured symbol → Todo | **Exact, always**: `by_symbol(sym).is_none()` on Task items | Cheap | obsidian-tasks: silent, config report only | None |
| (ii) NonTask-kind still a Task | Exact: `status().kind() == NonTask` | Cheap | legitimate configured type | None |
| (iii) `[]`/`[xx]` → plain text | **Not derivable** — rejected markers flushed into raw, item is Plain; needs parser instrumentation or heuristic re-scan | Medium | none anywhere; linters actively avoid | None (rumdl sees plain list) |
| (iv) tag-filtered → Checkbox | kind visible, **symbol lost** (unit variant) | Medium (instrumentation) | obsidian-tasks global filter, silent | None |
| (v) `📅 2026-13-45` invalid date | **Exact & cheap**: glyph survives in `clean_text` (Skip leaves it) — same trick as obsidian-tasks' "Finding unread emojis" | Cheap, no instrumentation | obsidian-tasks deliberately silent (docs `:3505`) | None |
| (vi) config-time errors | Already exhaustive: `deny_unknown_fields`, `ProhibitedStatusSymbol`, `InvalidTagFilter` | Done | n/a | n/a |

Structural fact bounding all diagnostics: **`ListItem` is line-anchored only** (`lists.rs:40-48`) — symbol-level underlines need marker re-scan (marker is item-leading, so cheap from line text) or span instrumentation; this is a ticket-25 requirement. Also: **no publish channel exists yet** — note diagnostics require 25 first.

---

## Part VI — R3: Traces semantics end-to-end

### VI.1 Classification-state inventory (C5, full)

| # | State | Evidence | Detectable | Severity recommendation | Intent evidence |
|---|---|---|---|---|---|
| 1 | Unconfigured symbol → synthetic Todo | `task.rs:142-152`, comment "for diagnostics" `task.rs:134-139`, test `list.rs:809-821` | with `TaskConfig`: `by_symbol()==None` | **Information/Hint** — diagnostic explicitly anticipated by the code comment and grilling-session notes | Intentional behavior (spec stories 4-5: unknown markers preserved as todos); diagnostic reserved |
| 2 | NonTask-kind still Task | `list.rs:182-204` never reads `kind`; docs claim otherwise `task.rs:289-291`, `raw.rs:152-153` | Exact | See VI.2 | **Conflict**: docs vs code+spec |
| 3 | `[]`/`[xx]` → plain | `marker.rs:74,78`; raw preserved `marker.rs:578-587` | Shape re-check on Plain+raw | **No diagnostic by default** (optional Hint) | Intentional (CommonMark/Obsidian rules, `marker.rs:3-4`) |
| 4 | Tag-filtered → Checkbox | `list.rs:199-200` | Exact by construction | Off by default (noisy), at most Information | Intentional (spec stories 18/24/25, `spec.md:91-96`) |
| 4a | ↳ emoji fields on such a Checkbox | `TaskFieldEmojis::Include` gates on marked, runs before classification (`list.rs:94-99`, `:172`) → `fields` has `due` but `as_task()` is None (test `query/results.rs:1130`) | Exact | Document for hover | Design consequence |
| 5 | Invalid date shorthand | `lexer.rs:217-248` Skip; test `list.rs:1479-1489` | Re-tokenize line (TaskScan `pub(super)`, unreachable) | **Defer to 18?** — see VIII.6; 18 is silent on it, so 23 must claim | Intentional skip |
| 6 | Invalid priority (emoji not in table / bad field) | not tokenized; inline swallowed `.ok()` `parser/task.rs:96` | field value only | Warning for field-value case | `.ok()` = deliberate silence |
| 7-10 | `[x]`/`[X]` both Done; VS16 variants; mid-line brackets plain; bare marker valid | `task.rs:382-415`, `marker.rs:417`, `list.rs:775-806` | n/a | Not divergences — pinned by tests/spec | Intentional |

**Span caveat**: item spans computed then discarded (`list.rs:174`) — every task diagnostic is line-granular until ticket 41 resolves span persistence; marker column is re-derivable from line text.

### VI.2 NonTask discrepancy (C9)

- Doc comments claiming NonTask "never becomes a Task": `src/task.rs:289-291` and `src/config/raw.rs:152-153`.
- Runtime: `kind` consulted only for `completed()` (`task.rs:303-311`) and display (`results.rs:379`). Consumers that ignore it: classification (`list.rs:182-204`), `Note::tasks()` (`model.rs:178`), Query `Mode::Tasks` (`service.rs:249-255`), CLI `traces task`, template `tasks` helper, **and `fully_complete` rollup** (NonTask → incomplete → blocks parent rollup, `list.rs:301-309`).
- Authoritative spec `.scratch/task-system/spec.md` never says NonTask items aren't Tasks (classification = marker scanner + tag filters, `spec.md:27-28`; NonTask only has a completion mapping `spec.md:242`).
- **Verdict recommendation: (a) inherit today's behavior, fix the two doc comments** (code + spec agree; docs are the anomaly). Option (b) (NonTask ⇒ not-a-Task) is a task-system semantic change → separate ticket, not LSP ticket 23; if pursued, decide Checkbox-vs-Plain and record the rollup change.

### VI.3 Config lifetime in a long-running LSP (C18)

- Capture: config loaded once per process; `TaskConfig` cloned into `IndexerService` (`index/service.rs:35-47`); parser borrows `&TaskConfig` per file.
- **Load-bearing insight**: classification is baked into `Note.lists` at parse time and persisted in redb ⇒ a mid-session `[tasks]` change invalidates **every parsed Note** — correct response is full `IndexerService::build()` reparse + re-persist + `Arc` swap, **not** ticket 13's single-file `RefreshPlan`.
- Already decided elsewhere: ticket 10 (`workspace/didChangeWatchedFiles` client-side watching — `.traces/config.toml` fits); ticket 31 (open) owns `didChangeConfiguration` vs file-watch and config-edit intelligence.
- **Trust gotcha**: out-of-band config edits hit Companion-Hash → `ConfigTrustStatus::Stale` → load fails (`src/config/trust.rs:129-141`, `error.rs:107-114`) until re-grant — 23/31 must decide how the LSP surfaces this.
- **Consistency requirement**: completion, diagnostics, and index classification must read the *same config generation* — house `TaskConfig` in the ticket-10 analysis-host facade; swap `Arc<WorkspaceIndex>` + config atomically.
- `[tasks.diagnostics]` toggle does not exist; `[schemas.diagnostics]` precedent (ticket 20: `enabled`+`debounce_ms`) is decided-but-unimplemented. Adding one later has `deny_unknown_fields` back-compat cost → flag for 31, 23 defers.

### VI.4 Shared predicates (C19)

Must consume, not re-derive: `TaskStatusMap::resolve`/`by_symbol` (`pub(crate)` `task.rs:127,142`), `TaskConfig::statuses()` (`pub(crate)`), `tag_filters()` (`pub`), `Note::tasks()`/`descendants` (pub), `ListItemType` accessors + `TaskListItem` fields (pub), `is_fully_complete` (precomputed). Barriers: **the tag-filter classification predicate is inlined at `list.rs:185-190` and must be extracted into a named shared function** (the single most important classification rule exists only as inline code); `TaskScan` is `pub(super)` — unreachable outside `note::parser`, only path is re-running `parse_markdown`. Visibility ↔ ticket 34: single-crate `src/lsp/` module → `pub(crate)` fine as-is; workspace split → exact widening set identified (resolve/by_symbol/statuses/completed/by_name/by_type).

### VI.5 Config vs document boundary (C8)

Already config-load errors (fail whole `Config`): unknown `[tasks]` keys (`deny_unknown_fields`, `raw.rs:112`), invalid tag filters (`model.rs:685-696`), `ProhibitedStatusSymbol` (`task.rs:199-209`), trust stale (`error.rs:107-114`). Latent never-surfaced: `TaskError::InvalidPriority`/`InvalidDateType` (swallowed with `.ok()`). **Genuinely document-level** (23 candidates): unknown status symbol, invalid marker shape, invalid date shorthand, tag-filter miss, unparsable priority field.

Division of labor: **23 specifies which findings exist + relative severity ordering + completion source + outline nesting; 25 owns publishing/severity taxonomy/debounce/push-vs-pull; 31 owns config reload + toggles; 18/27/21 as noted elsewhere.**

---

## Part VII — R4: Tasks as symbols (bullet c)

### VII.1 Precedent (C11): absent everywhere, by explicit design

| Peer | documentSymbol | workspace/symbol | Evidence |
|---|---|---|---|
| rumdl | headings only ("Markdown has no functions or classes, so rumdl exposes the document's heading outline") | headings | `lsp_rvben-rumdl-digest.txt:71515-71519` |
| Microsoft markdown-ls | TOC headings `String` + optional link-defs `Constant` | flat `String` | `…digest.txt:4590-4730`, `:4662-4680` |
| Marksman | headings + **tags nested as children** | headings + tags | `…digest.txt:12472-12533` |
| Markdown Oxide | headings `Struct`; research: "No lists/indented-lists in Workspace Symbols" | headings | `tool-markdown-oxide.md:47` |
| zk | none at all | none | `tool-zk.md:13-14` |

Client evidence: VS Code defines the Markdown outline as "the file's header hierarchy" (official docs); mjbvz: "VS Code itself targets standard markdown which does not include task lists" (vscode#201669); VS Code task rendering closed as extension-candidate (vscode#67596); task extensions build custom views, not documentSymbol. Mitigation: per-SymbolKind Outline toggles (`outline.showArrays` etc.) let users hide tasks independently of headings. No direct evidence exists of clients rendering mixed heading+task outlines well *or* badly — say so honestly.

### VII.2 Spec & construction (C12)

- Only hard rule: `selectionRange` ⊆ `range`. No children-containment, no ordering, no homogeneity requirement — **mixed kinds legal** (precedent: MS link-defs under headings; Marksman tags under headings).
- `name` must be non-empty → empty-text task needs fallback (rumdl `UNTITLED` precedent, `:71735-71738`).
- `detail`: none of the three heading-peers populate it; Traces would be first — decide deliberately.
- `SymbolKind`: `Array(18)` floor-safe (clients guarantee 1–18 without `valueSet`) and independently hideable; `String(16)` matches how every peer renders headings. No check-square kind exists.
- Flat `SymbolInformation[]` fallback cannot carry hierarchy — nesting depends on `hierarchicalDocumentSymbolSupport` (ticket 29).
- **Nesting must be built from `depth`, never `parent`**: parent-line ambiguity verified — a child on the same physical line gets `parent == line` (`parser.rs:497-499`, pandoc-verified `- - foo` / `  - bar`); the codebase's own `descendants_of` uses depth+contiguity (`lists.rs:643-648`).
- Heading section extents not stored but derivable O(n) at request time via a level stack (rumdl `:71560-71596`, MS `:4706` — no persistence needed).

Options, all spec-legal: (a) list-tree by depth [derivable today]; (b) under enclosing heading section [needs request-time stack]; (c) tasks-only tree [leanest]; (d) omit tasks [all five peers' choice].

### VII.3 Range fidelity (C13)

Line-granular satisfies the spec **with zero AST change**: parent range = start line..(last descendant end line+1); selectionRange = start line..start+1. Byte precision would amend ticket 11 §1 + intersect ticket 41 (ListItem is serde-persisted, `src/index/store.rs:1886`) → out of 23's remit unless char precision is demanded. Recommendation: explicitly pick line-granular now.

### VII.4 Cost & workspace symbols (C14)

Document symbols: all notes memory-resident (`src/index/entry.rs:134-160`), subtree build O(items). Ticket 33 has a document-symbol target (<10ms) **but no workspace/symbol latency row — flag as a 33 gap**; also 33's LRU-AST-tier plan doesn't include tasks in always-resident — workspace task symbols would acquire reload costs (task rows join always-resident, or 33 prices reloads). Workspace/symbol O(total items) per request is exactly what every peer does; user-invoked, not hot. Existing `QueryService::tasks` could serve it but a direct `WorkspaceIndex` walk over `note.lists()` is leaner (note the query source-resolution linear scan, `service.rs:132-139`).

### VII.5 Ownership boundary (C10)

Literal overlap: 23:14 claims "tasks as document symbols / task outline / sub-task nesting"; 27:10 claims "Document symbols/outline: heading hierarchy … plus, **optionally, task** … structure". Precedent (ticket 16): product content lives in feature tickets; protocol mechanics live in capability tickets. Proposed split:
- **23 decides**: whether tasks appear at all; vault-wide outline question; nesting model (a–d); Plain/Checkbox children treatment; `name`/`detail` content.
- **27 decides**: heading outline construction, tree building, range-precision policy (consuming C13), flat fallback, registration/label, perf, client coexistence.
- **Edge: prose cross-reference, not `Blocked by`** (tracker supports only blocking; 27:14 sets the precedent for optional-input prose).
- **New gap found**: Marksman disables both symbol providers when client is VS Code (`lsp_artempyanykh-marksman-digest.txt:11147-11149`) because VS Code's *built-in* markdown provider already supplies outline+workspace symbols. **No Traces ticket addresses coexistence with VS Code's built-in provider** — flag for owner assignment (27 mechanics / 32 boundary / note on 29).

---

## Part VIII — R5: Emoji markers, hover, validation (bullet d)

### VIII.1 obsidian-tasks Auto-Suggest (C15) — behavioral gold standard

- **The emoji token itself is a popup item**: `addField` pushes `{displayText: "📅 due date", appendText: "📅 "}` (`obsidian_obsidian-tasks-src-digest.txt:23418-23424`); keyword table at docs `:2527-2595` (all 6 date-ish + 5 priority + 🔁🔁/🏁/🆔/⛔ tokens Traces doesn't have).
- Selecting a date emoji re-opens with computed dates (`today/tomorrow/Sunday…/next week/…`, src `:23530-23543`), displayed as `tomorrow (2022-07-12)` but **inserted as ISO** (`dateExtractor`, src `:23509-23515`); NL parsing of typed values with `forwardDate` (src `:23553-23564`).
- Trigger: whole-line query (src `:23027-23052`), gated by `canSuggestForLine` (recognized task, status ≠ NON_TASK, global filter, cursor in description). Suppression: field already present → hidden (src `:23419`); any priority present → priority options hidden (src `:23431-23433`, docs `:2276`); menu is "smart: only valid options".
- Settings: `autoSuggestMinMatch` default **0** (pops before any typing), `MaxItems` default 20 (docs `:2620-2634`, src `:2905-2906`).
- **Invalid dates: nothing** — no underline, no warning; opt-in via query blocks only (docs `:3505`, `:6874-6880`); auto-suggest never validates (src `:23380-23629`); Reading-view renders `Invalid date` (docs `:4821-4832`).
- Task-line date format **not configurable — `YYYY-MM-DD` only**, cannot be wikilinks (docs `:3515-3521`).

### VIII.2 LSP/editor ecosystem (web, per methodology rule)

No Markdown LSP/extension completes or hovers obsidian-tasks-style emoji tokens. Closest true-LSP precedent: **taskpaper-ls** (Zed) — due-date diagnostics (overdue warning/due-today info), inlay hints, hover on `@due`/`@done` showing relative dates, `@`-completion with date suggestions (github.com/ickc/zed-taskpaper). Others: Chevron Lists (`@`/`!` completion + due decorations), markdown-todo-vscode (date squiggles), markdown-plus.nvim (toggle *inserts* `✅ YYYY-MM-DD`), TaskLite/Tasks Companion (Obsidian). rumdl/Marksman/MO/MS: nothing on task emoji (MO "tasks" hits are unimplemented roadmap prose). Consistent with research 21's finding: **design from obsidian-tasks' behavior, not an LSP parity bar that doesn't exist.**

### VIII.3 Token scope & the three nested scopes (C16)

Lexer `ItemToken` has exactly four variants (`lexer.rs:108-149`): `Priority` (always lexed, extracted for Tasks only, stripped from clean text everywhere), `Tag`, `Date` (gated by `TaskFieldEmojis`; emitted only when followed by a *parseable* ISO shape + non-alphanumeric terminator), `Field`. Not present: 🔁, 🏁, 🆔, ⛔, 👤, and no marker token (markers are `MarkerAccumulator`).

| Scope | Gate | Effect | Site |
|---|---|---|---|
| S1 clean-text stripping | *every* list item | priority/date emojis + wrapped task-key fields removed from display | `list.rs:180-181`, `task.rs:113-148` |
| S2 field conversion (duality) | status-marked | `Date` token → same `NoteFieldValue::Date` as `[due:: …]` | `list.rs:94-99` |
| S3 task extraction | marked + tag-filter match | populate `TaskListItem` | `list.rs:182-204` |

**Two hover traps**: plain bullet (glyph stripped, semantically inert — test `list.rs:1623-1631`) and filtered Checkbox (fields have `due`, `as_task()` is None). Same visible text, three different meanings.

**Proposed seam statement (for adoption/amendment)**: 23 owns task-context emoji-token intelligence (glyphs-as-tokens, gating S1–S3, token completion + ISO values, hover content, invalid-task-date diagnostic proposal); 18 owns date *value* semantics (DateValue, NL parsing, date-note linking — reusable as candidate source, never touches task lines); 19 owns inline-field *spelling* mechanics (`[due:: …]` completion/hover, never `📅`); 20 owns schema-typed date validity (frontmatter-only, doesn't reach task lines; its "syntax-only via chrono" sets the bar); 25 owns severity/debounce/namespacing; 26 owns hover mechanics/MarkupContent; 24 owns dispatch/triggers. One-liner: **"23 decides what a token means and says; 18 decides what a date is; 19 decides how a field spelling completes; 20 decides when a date is invalid under schema; 25 decides how loudly we complain; 26 decides how it renders; 24 decides how the cursor gets here."**

**Unowned gap to assign**: date-*value* completion after `[due:: ` — 19's `:` trigger is frontmatter-gated, 20 completes file fields only, 18 completes wikilink date targets, 23 would only claim post-`📅`. Either 23 claims both spellings (same `NoteFieldValue::Date`) or record as a known gap.

### VIII.4 Hover (C17)

No peer editor hover on task tokens (obsidian-tasks: Reading-mode only, ❌ in Source/Live Preview, docs `:2962-3007`; digest LSPs: nothing; taskpaper-ls closest). Content spec (= 23's to decide):

| Token | Content | Data source (already exists) |
|---|---|---|
| Status symbol `[x]`/`[/]`/`[?]` | name + kind + "configured via `[tasks]`" + completion tri-state | `TaskStatus::{symbol,name,kind}` via `resolve` on `TaskListItem::status` |
| Date emoji | slot name, parsed ISO, relative form ("in 3 days"/overdue), `🗓≈📅` note | `TaskListItem::dates()` |
| Priority emoji | level name + rank + "Normal = no emoji" | `TaskListItem::priority()` |
| Invalid/unconsumed date emoji | "not extracted; searchable by ISO only" hint | needs secondary scan (§ VIII.5) |
| Same glyphs on plain bullet / filtered Checkbox | trap resolution — open call | S1/S2/S3 scopes |

Mechanics split: 26 owns MarkupContent kind/capability/dispatch registry; 23 owns content + gating rule; 25 owns severity if promoted to diagnostic (hover-only informational hint always available — 18's precedent `issues/18:32`).

### VIII.5 Invalid-date diagnostic seam

Peer evidence split: obsidian-tasks deliberately silent (docs `:3505`); taskpaper-ls and markdown-todo-vscode ship date diagnostics. The failure is real & silent in Traces (glyph stripped, no token → query misses the task, zero feedback). **Feasibility**: the lexer erases the evidence (`Filter::Skip`, `lexer.rs:233-245`) → needs a small secondary scan (find date-emoji occurrences, check parseable ISO follows). **Recommendation: extract `task_date_callback`'s predicate into a shared helper used by parse, hover, and diagnostics so they can't drift** (~zero cost). Seam: 23 proposes rule/span/message/scope (marked-items-only vs all-lines = open call); 25 slots severity/code/debounce; code actions deferred per 19's precedent.

### VIII.6 Ticket-18 alternative verdict: **MOOT**

18's Answer (9 decisions + what-not-to-build) and research contain zero mentions of task lines/checkboxes/emoji; 18 is resolved; nothing in it invites reopening. Reusable from 18: ±7-day candidate generator, NL parser, `DateValue` — cite and reuse; no amends-link needed, at most "Relates to".

### VIII.7 Emoji completion mechanics (leanest)

Offer emoji token itself (obsidian-tasks parity): label `📅 due date`, insert `📅 `, keyword filterText. Insert ISO values; relative phrases as labels only. Suppression must be **token-based**, not obsidian-tasks' `line.includes(symbol)` — otherwise the menu hides the fix precisely when the existing emoji is invalid (not a token). Gate on task lines (S2/S3-aligned), not plain bullets. Emoji tables stay code constants. **Cheapest correct implementation: re-lex the single cursor line at request time** (`tokenize_item_text(line, Include)` — one logos pass, sub-µs, yields spans → `LineIndex::byte_to_utf16_cu`); do **not** reuse parse-time spans (dropped at `end_item` AND wrong — `ListText.raw` offsets drift from source with markup). Avoid: whole-document re-tokenize per keystroke, persisting item-token spans, runtime emoji→slot maps.

---

## Part IX — Consolidated recommendations

### Settled by evidence (confirm, don't debate)

1. **Restate the Question first** — unknown symbol → synthetic Todo (reserved "for diagnostics"), NonTask orthogonal, tag-filter drives Task/Checkbox, 18-alternative moot (Part 0).
2. **No completion precedent exists** — green field; status changes elsewhere are commands/cycles. Traces would be first (IV.1).
3. Completion uses `text_edit`, inner-span-only range, structural gating (bullet+`[`), never bare `[` (#1509); exclude NonTask; read statuses at request time — zero keystroke cost (IV.2–3).
4. **No peer emits per-note task diagnostics** — no wording/severity convention to inherit; don't spend the grill choosing among conventions that don't exist (V.1).
5. **rumdl non-overlap is structurally guaranteed** (flavor-blind GFM parse) for every candidate state; record as settled for 32; leave only same-line-different-message adjacency (MD064/MD076) to 25's design. Never `MD…` codes (V.2).
6. **Config-time errors already exist and are exhaustive** — don't re-diagnose config in note files (VI.5).
7. **NonTask: inherit behavior, fix the two doc comments**; semantic change = separate ticket (VI.2).
8. **Classification is baked at parse time** → `[tasks]` change = full reparse + atomic `Arc`-swap of index+config, not single-file refresh; trust-stale flow must surface (VI.3).
9. **Extract the tag-filter predicate** from `list.rs:185-190` into a shared named function; expose a `TaskStatusMap` iterator (VI.4, IV.3).
10. **Symbols: all five peers are headings-only**; task symbols would be an intentional extension; mixed trees are spec-legal; **nest from `depth`, never `parent`**; line-granular ranges suffice with zero AST change; empty-name fallback required (VII.1–3).
11. **Emoji: offer the token itself, ISO values, token-based suppression, task-line gating**; re-lex the cursor line at request time; share the date-emoji predicate across parse/hover/diagnostic (VIII.1, VII.7, VIII.5).
12. **Zero new crates** (Part II).

### Open judgment calls (the actual grill)

1. First-mover risk: ship inline symbol completion as experimental / behind a flag? Bare `- [` incomplete-marker case: insert `x] ` or v1-defer? Label/kind/sort/preselect; Todo's space-symbol `filter_text`; `x` vs `X` dedupe; predicate packaging (widen `scan_marker_prefix` vs colocate). **Trigger seam with 24.**
2. **Should state (i) warn at all?** Cheap + exact + reserved-for-diagnostics, but common (`- [?]` is normal in Obsidian vaults) — frequency × silence-precedent argues for config-adjacent surfacing (obsidian-tasks' on-demand report pattern, which also fits the existing CLI+miette surface and sidesteps 25) rather than per-note noise. Decide *form* before severity. State (iii) `[]`/`[xx]`: error or formatting accident (no precedent; linters avoid it)? State (v) invalid date: in 23 (18 is silent) or out? Severity? State (iv) tag-filter miss: default off?
3. **NonTask open option** (b) recorded but out of 23's remit (A8).
4. **Symbols top-line yes/no**; nesting model a–d; Plain/Checkbox children; workspace/symbol now or defer (A3 — 33 has no target); `SymbolKind` Array vs String; `detail` content; **VS Code built-in coexistence owner** (unowned gap); C10 edge wording.
5. **Emoji**: invalid-date diagnostic yes/no/severity/scope; bullet-vs-checkbox hover behavior; trigger plumbing with 24 (no ASCII trigger char — invoked/incomplete gated, or space-trigger post-`📅`); date candidate set (fixed 12 vs 18's ±7 hybrid + NL parser); claim `[due:: <value>]` value completion or record the gap; relative-hover "today" primitive (`src/date.rs` owner); assign `mod.rs` doc fix.
6. **A1 (status toggling) / A5 (task hover scope) / A6 (tag-filter in Decide list) / A7 (graph edges to 24/25/27)** — wording/graph decisions for the session itself.

### Boundary summary — what ticket 23 decides vs the rest of the map

**23 decides**: checkbox-symbol completion product shape (predicate, offer content, source-of-truth = `TaskStatusMap`) as input to 24; which task diagnostics *exist* + relative severity (25 slots them); task outline content/nesting (27 owns mechanics); emoji-token completion/hover content + invalid-date proposal; the classification-state semantics LSP features inherit; NonTask doc fix; tag-predicate extraction; config-generation atomicity requirement (for 31).
**Defers**: 24 dispatch/triggers · 25 publishing/severity/debounce/namespace · 26 hover mechanics · 27 symbol capability/range policy/heading outline · 18 date values/NL parsing · 19 inline-field spelling · 20 schema-typed dates · 31 config reload/toggles · 34 visibility widening · 41 span persistence · 33 workspace-symbol latency target.

---

## Part X — R6: TUI/CLI task-management systems (gap-fill)

Supplementary bundle filling the TUI/CLI gap; no local digests or `.rust-docs` crates exist for these tools (both verified empty) — all web-sourced: official docs/mans + first-party source. Systems surveyed: Taskwarrior (+ taskwarrior-tui), todo.txt ecosystem (todo.txt-cli + 6 TUIs), Org-mode, dstask, tick/tickit, Todoist CLI, Super Productivity, Obsidian Task States plugin.

### X.1 Status models across the family

- **Taskwarrior**: fixed closed enum `pending|waiting|completed|deleted|recurring` ([task.1](https://taskwarrior.org/docs/man/task.1/), [Terminology](https://taskwarrior.org/docs/terminology/)); transitions are named commands (`done`/`undo`/`modify`), never cycles; write-boundary validation (import historically coerced `status:"gibberish"` → `pending`, later validated — [TW-1666/taskwarrior#1690](https://github.com/GothenburgBitFactory/taskwarrior/issues/1690)); config-typo detection at inspection time (`task show`, [Configuration](https://taskwarrior.org/docs/configuration/)). All user extensibility pushed into **UDAs** (`uda.<n>.values=A,B,C` ordered closed vocabulary; orphan UDA data preserved-but-unmanipulable + `ORPHAN` virtual tag + `task udas` on-demand listing — [taskrc.5](https://taskwarrior.org/docs/man/taskrc.5/), [UDAs](https://taskwarrior.org/docs/udas/)) and reports (`description/columns/labels/sort/filter` quadruple — [Reports](https://taskwarrior.org/docs/report/)); priority was demoted from core to UDA in 2.4.3 — explicit evidence of the principle *user-configurable leaves the status enum; the enum stays closed*.
- **todo.txt**: exactly ONE completion state (`x ` prefix, case-sensitive, format README Rule 1); validation only at command input (`pri` dies on non A–Z — todo.sh source); no status vocabulary exists, so "unknown marker" is structurally inapplicable. Only "status-like" cycling found: **priority** cycling in tuxedo (`A→B→C→·`).
- **Org-mode** (the structural precedent): configurable state sequences `#+TODO: TODO(t) | DONE(d)` — `|` bar separates action states from DONE states, post-bar keyword "must always mean DONE" ([Workflow states](https://orgmode.org/manual/Workflow-states.html), [Per-file keywords](https://orgmode.org/manual/Per_002dfile-keywords.html)); cycle `C-c C-t` = primary verb, **fast-key picker and `completing-read "State:"` explicitly framed as alternatives** ([TODO Basics](https://orgmode.org/manual/TODO-Basics.html), [Fast access](https://orgmode.org/manual/Fast-access-to-TODO-states.html)); cycle set = config order and **includes unmarked** as a state; fast-selection grid renders `[t] TODO` — bracketed key + word side-by-side (org.el, elpa-diffs msg01179). **Checkbox/TODO duality**: checkboxes are `[ ]`/`[-]`/`[X]`, hierarchical rollup via `[/]`/`[%]` cookies, "Checkboxes are not included in the global TODO list" — exactly Traces' Task-vs-Checkbox split + `fully_complete` ([Checkboxes](https://orgmode.org/manual/Checkboxes.html)).
- **dstask**: closed enum with **hard-validated transitions** (`IsValidStateTransition` errors, refuses `resolve` with incomplete checklist) — the strictest boundary-validation found.
- **Obsidian "Task States" plugin** (closest to Traces' map): 6 cyclic markers, names renameable but markers fixed, drag-reorder sets cycle order, TODO first/DONE last; **unknown marker silently normalized to `[ ]` on next click** (obsidianstats.com/plugins/task-states).
- **Cycle pattern boundary**: binary toggle in DB tools (taskwarrior-tui `d`/`u`, Obsidian core `Cmd+L`, todo.txt TUIs); vocabulary cycle only where the *document carries a configurable symbol vocabulary* (Org, Obsidian plugins) — and there cycle order is always config order.

### X.2 Done-state / hover content

Field-style precedent for hover: Taskwarrior `end:` date + `task info` change log; Super Productivity `doneOn` timestamp; todo.txt inline completion date after `x `; Org LOGBOOK drawer. **Every history is stored out-of-line; nobody renders transition history inline** — a Traces hover showing "completed <date from ✅>" is field-style (precedented); multi-step transition history has zero precedent and nowhere to persist.

### X.3 Transfer answers

- **(i) Offer content/ordering**: `[symbol] Name` label doubly precedented (Org `[t] TODO`, obsidian-tasks `[ ] Todo`); **ordering = config/insertion order universally** (Org sequence, UDA `values` order, Task States drag order, obsidian-tasks cycle) — alphabetical/hash order appears nowhere. Upgrades §IV.3's `sort_text` from determinism fix to **product requirement**: `sort_text` = `TaskStatusMap` `kinds` Vec insertion order.
- **(ii) Cycling (A1)**: ecosystem verdict — **cycle/toggle-command is the primary verb, pick-from-list secondary**; no tool puts status change in inline completion; cycles carry side effects (obsidian-tasks toggle edits done-date) → **A1 resolves to ticket 27 (code actions/commands)**, with a recorded invariant: *cycle order = config insertion order = completion `sort_text` order*. Open: cycle steps over symbols (x→X two stops) or kinds (one Done stop)? Org cycles per-keyword, Taskwarrior per-role — both precedents exist.
- **(iii) Unknown-status diagnosis — four patterns, none inline-per-note**: (1) boundary validation at write/config time (Taskwarrior import/`task show`, dstask hard errors, todo.txt `pri`); (2) **silent normalization** (Taskwarrior's coerced `pending`, Task States → `[ ]`, **Traces' synthetic-Todo = this pattern**); (3) on-demand report (`task udas`/`ORPHAN`, obsidian-tasks `StatusSettingsReport`); (4) silent (Org unbound fast key → silent quit; all todo.txt TUIs). Strongly confirms Part IX open-judgment #2: prefer config-adjacent/on-demand surfacing + hover over per-note diagnostics; the existing fallback is *the* ecosystem pattern, not a wart.
- **(iv) Closed vs open**: Taskwarrior's closed enum is about *database workflow states*, doesn't transfer to document markers; Org (open vocabulary + closed role bar) and Task States support Traces' open map. Convergent principle: **surface vocabulary open, role taxonomy closed, commands operate on roles** — Traces already matches (open `[tasks].statuses` + closed `TaskStatusType`). No evidence for closing the map.
- **(v) `traces task` CLI cross-refs (record, don't decide)**: report quadruple + urgency-style composite sort as presentation model; taskwarrior-tui's CLI/TUI single-config-source → `traces task` display and LSP completion must read the same `TaskStatusMap`; introspection subcommands (`task udas`/`_columns`/`show` → analog `traces task statuses`) as the config-adjacent surfacing vehicle for state (i)/(ii); virtual tags as query-layer vocabulary for kind-based filters.

---

## Part XI — R7: Broader PKM systems (gap-fill)

Supplementary bundle: Logseq (file + DB graphs), Roam, Notion, TriliumNext, Capacities, RemNote, Zettlr, Obsidian-core, Org-mode origin, emoji-grammar interop. Official docs primary (raw GitHub docs repos where sites are JS-rendered); forum/GitHub evidence flagged as secondary.

### XI.1 Per-system highlights

- **Logseq file graphs**: two hard-coded workflows `LATER→NOW→DONE` / `TODO→DOING→DONE` + typed-only CANCELED/WAIT/IN-PROGRESS; marker regex is a **hard-coded alternation** (graph-parser `exporter.cljs:444-451`); `#+TODO:` custom keywords **not supported** (feature request open since 2020); unknown keyword → plain text, query silently returns 0 rows, no warning; `[#A]/[#B]/[#C]` priorities; task overview = query views (`{{query (task …)}}`), never the outline.
- **Logseq DB graphs** (closest structural analog): `#Task` blocks with `Status`/`Priority`/`Deadline`/`Scheduled`; default statuses Backlog/Todo/Doing/In Review/Done/Canceled — extendable but built-ins undeletable; **`Checkbox state mapping`** — a property's `Available choices` mapped to checked/unchecked — the only PKM feature *shape-identical* to Traces' symbol→status map, and it's configured in a settings UI with a closed choice list (db-version.md L118-130).
- **Roam**: official docs **404** (verified); secondary only — `{{[[TODO]]}}`/`{{[[DONE]]}}` toggle + sidebar query dashboards. WAITING/STRM markers and TODO/TASK/NOTE query **unverified**.
- **Notion**: Status property with **three immutable groups** (To-do/In-progress/Complete — "You can't change the three main categories"; part of the API contract), sub-categories customizable; sub-items unlimited nesting viewed in table/timeline; validation only at API boundary (400 on unknown status/ISO date); tasks never in any structural outline (TOC = headings); board view grouped by status is the task overview.
- **TriliumNext** (richest precedent): custom checkbox states config UI — per state Icon/Title/Identifier/Markdown symbol/Counts-as-completed/Color/**`Hidden from toolbar`** (deprecation without breaking old notes); "Two task states cannot share the same symbol"; **config validated at startup — toast + affected definition ignored**; deleted state → items fall back per Counts-as-completed; **source-level unknown-state handling**: unrecognized id preserved in saved markup, renders as anchor state, editing-only CSS class + hover tooltip `"(missing definition)"`, never written to file (`todo_list_multistate_editing.ts`); Outline tab = **headings and highlights only**; heavy task tracking routed to Kanban/Dashboard views.
- **Capacities**: optional status property (checkbox independent, "you can ignore the status property"), categories per status, title autofill parsing `!!`/`tomorrow 2pm` at input time with `Esc` to keep literal (the only PKM inline-metadata-entry precedent, and it ships a suppression key), progress ring on parents, task dashboard view.
- **RemNote**: two states + 3-press cycle (bullet→unfinished→finished→bullet). **Zettlr**: GFM checkboxes only, nothing distinctive. **Obsidian core**: "any character inside the brackets" (everything valid); core Outline plugin = headings only ⇒ **no native task outline in Obsidian either**.
- **Emoji interop**: the date-emoji grammar is consumed *outside Obsidian* — `taskspec-go` (documented emoji↔text field table, ISO 8601) and `vault-cortex` (reimplements serializer; calendar-invalid → stripped + null) ⇒ Traces' `TaskDateType::EMOJIS` matches a **cross-tool convention**, not a vault quirk.

### XI.2 Transfer answers

- **(i) Status vocabularies**: zero PKM offers symbol completion (cycles/commands/menus everywhere: Logseq `Cmd+Enter`, Trilium `Ctrl+Shift+Enter`, RemNote 3-cycle, Notion dropdown, Capacities click) — **Part IX.2 confirmed from the PKM side**. Logseq-DB's closed choice list corroborates that symbol→status maps are a *config* concern.
- **(ii) Diagnostics**: **no PKM emits per-note task diagnostics**; four strategies — silent-not-a-task (Logseq), impossible-by-schema (Notion/Capacities/Logseq-DB/Trilium-config), **render-fallback-with-data-preservation** (Trilium source: editing-only cue + tooltip), everything-valid (Obsidian core). Traces' symbol-preserving fallback is more diagnostic-capable than every peer, with no counter-precedent. **New form option for open-judgment #2**: Trilium's editing-only cue = "visible in editor, absent from file, silent to linters" — the cheapest middle ground between diagnostic and silence. Invalid dates: ecosystem-wide silence (obsidian-tasks documented, vault-cortex nulls, Notion API-only) ⇒ a Traces invalid-date diagnostic would be **first-of-kind in any PKM or LSP** — genuine open call, not follow-the-pack.
- **(iii) Symbols/outline**: **headings-only everywhere** — Trilium Outline tab, Obsidian core Outline, Notion TOC; task overviews are always *dedicated views* (Logseq tables/queries, Notion board, Capacities dashboard, Trilium Kanban, Roam sidebar queries). Subtask nesting exists structurally (depth/relation) but is never surfaced through a symbol API. ⇒ R4's LSP finding now extends to PKM UIs; "task outline" as a *view* maps to Traces' existing `Query Mode::Tasks`, not to `documentSymbol`; tasks-in-documentSymbol = intentional extension with **zero precedent in either world**. Nesting by depth-in-flat-list everywhere supports "nest from `depth`, never `parent`".
- **(iv) Emoji markers**: cross-tool convention (XI.1) with zero validators anywhere ⇒ token completion/hover remains first-mover; single-shared-predicate recommendation (VIII.5) matches how consumers do one parse for both read paths.
- **(v) Relationship to Part IX**: nothing contradicts; five items externally confirmed (IX.2, IX.4, IX.6 via Trilium's startup toast, IX.10, IX.11).

---

## Part XII — Amendments to Part IX after R6/R7

All prior settled items stand; the gap-fill adds:

**Strengthened (now multi-world evidenced):**
1. IX.2 (no completion precedent) — confirmed across TUI/CLI *and* PKM worlds; cycling is universally a command with pickers secondary.
2. IX.4 (no per-note diagnostics) — confirmed across both; the four-pattern taxonomy (boundary-validation / silent-normalization / on-demand-report / silent) now backs open-judgment #2, with Trilium's editing-only cue as a named third option alongside "diagnostic" and "silent".
3. IX.6 (config-time errors) — Trilium's startup-toast-and-ignore is external corroboration of the config/document split.
4. IX.10 (headings-only outline) — now confirmed in PKM *UIs* too; "task outline"-as-view ⇒ `Query Mode::Tasks` is already the vehicle (refines A3).
5. IX.11 (emoji grammar) — cross-tool convention (taskspec-go, vault-cortex), standards-adjacent.

**New decisions/requirements supplied:**
6. **A1 closes**: cycle/toggle belongs to ticket 27 (code actions) — with invariant *cycle order = config insertion order = completion `sort_text` order*; and `sort_text` = `kinds` Vec order is a **product requirement**, not a determinism patch (config-order is universal; nobody sorts alphabetically).
7. **Label `[x] Done` doubly precedented** (Org fast-grid `[t] TODO`, obsidian-tasks `[ ] Todo`).
8. **Open-map + closed-kind design is ecosystem-convergent** (Org's role bar, Taskwarrior's closed-enum-with-UDAs is a database concern that doesn't transfer) — no evidence for closing `TaskStatusMap`.
9. **Hover**: done-date field-style content precedented (Taskwarrior `end`, SP `doneOn`); transition-history rendering has zero precedent — don't build it.
10. **`traces task` cross-refs recorded** (report quadruple, single-config-source with the LSP, introspection subcommands as config-adjacent surfacing vehicle) — for the CLI, not for 23's decisions.

**Still open (enlarged by the gap-fill, unchanged in kind):** form of unknown-symbol surfacing (now three options: diagnostic / editing-only cue / config-adjacent report); invalid-date diagnostic (now known first-of-kind); cycle symbols-vs-kinds question (belongs to 27); offer "unmarked/remove marker" item (Org's cycle includes unmarked; no completion precedent).

**Could-not-verify register (R6+R7):** Roam primary docs 404; Taskwarrior `unknown` status not in current docs (set is the five listed); Org invalid-fast-key warnings unverified (silent in source); Logseq "no warning" is inferred not documented; Notion in-app validation inferred; universal negatives bounded by survey scope.
