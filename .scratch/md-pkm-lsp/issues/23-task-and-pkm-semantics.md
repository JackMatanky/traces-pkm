# Task & other PKM-specific semantics

Type: grilling
Blocked by: 01, 02
Status: resolved

## Question

Grounding: `ListItem` (`src/note/lists.rs:64`) already models `kind: ListItemType::Task(TaskStatus)` with `text`, `children`, local `fields`, and a line-only `position: ListItemPosition`. `TaskStatus`/`TaskStatusMap` are configurable via `[tasks]` in Config (checkbox-symbol→status mapping, tag-filter classification). Query DSL already has a dedicated `Query Mode::Tasks` (one row per task). Informed by Markdown Oxide/Marksman task handling if any (tickets 01, 02) and the obsidian-tasks parity findings folded into ticket 08.

Decide:
- Completion: task-status checkbox-symbol completion (informed by the project's configured `TaskStatusMap`, not a hardcoded set).
- Diagnostics: recognized-but-misconfigured task syntax (e.g. a checkbox symbol not in the configured map — is this even diagnosable, or silently non-task per existing `TaskStatusType::NonTask` semantics?).
- Symbols: are tasks surfaced as document symbols / a task outline, and do child sub-tasks nest under parents (using the existing `ListItemPosition.parent: Option<SourceLine>` relationship)?
- Whether task date-shorthand emoji markers (mentioned in `src/note/CONTEXT.md`) get their own completion/hover treatment, or fall entirely under ticket 18 (daily notes/date references).

## Answer

Grounding in this Question is wrong on two counts (corrected during grilling): `ListItem` is at `src/note/lists.rs:40-48` with `kind: ListItemType::Task(TaskListItem)` (`lists.rs:284`), hierarchy via `parent`/`depth` not `children`, and `ListItemPosition` was dissolved (commit `50bdc493`). Unknown checkbox symbols do **not** go NonTask — they resolve to a synthetic Todo preserving the symbol for diagnostics (`src/task.rs:142-153`); NonTask is a configured kind classification never consults. Full evidence: [research/23-task-pkm-semantics.md](../research/23-task-pkm-semantics.md) (Parts 0–XIV; XIV = four adversarial stress-tests).

Decisions (23 owns product content; mechanics deferred per seams below):

**1. Completion — checkbox symbols (23 content, 24 dispatch).**
- Mid-typing `- [` position-gated completion: buffer-local O(line) predicate (indent + bullet + ws + `[`), offer iff `scan_marker_prefix` state is `Incomplete` (`marker.rs:70-77,92-113`); `Rejected` falls through to 19's contexts; `Complete` → field/value layer. Accept edit range = after-`[`..cursor. Items from configured `TaskStatusMap` (add `iter()`, ordering = config insertion order = `sort_text`); `preselect` top; no unmarked/removal item; Invoked fallback + `TriggerForIncompleteCompletions` re-requests; no space trigger. Trigger registration and the shared completion-generation function are **24's** (24's Question owns the enumeration; every trigger claim here is a proposal into 24). `scan_marker_prefix` is module-private and `TaskScan` is `pub(super)` — widening rides 34.

**2. Completion — date values (23 owns candidates+content for both spellings).**
- Claim date-value completion for `📅 ` AND `[due:: ` — same `NoteFieldValue::Date` (`lexer.rs:30`); closes a gap 19 (keys only) and 20 (frontmatter only) provably don't cover. **Seam split: 19 = position predicate, 23 = candidates + value content, 24 = dispatch.**
- Candidates: obsidian's generic 12 as parity set (`today…next year`) + `autoSuggestMaxItems: 20`; ISO dates today ±7 (needs the new `today` primitive — input to 18/`src/date.rs`); 18's NL-v1 phrases as labels. 18's ±7 generator is design-only — if implemented, 23 consumes its algorithmic half; on-disk file matching excluded for task lines.
- Suppression = token-based show/hide keyed on the **slot across both spellings** (first-wins, emoji precedence `task.rs:62-79`); invalid occurrences stay visible (fix affordance) and accept emits a spanning `textEdit` (needs `byte_to_utf16_cu` — 11/19 prerequisite, currently absent).
- Emoji-token completion: label `📅 due date`, insert `📅 `, filterText keyword; stage-2 value list after accepting `📅 ` via `triggerSuggest`/isIncomplete session. Completion gates on **marker state (marked)**, not classified-task — filtered Checkboxes have emoji fields.

**3. Hover — content-gated, not blanket silence (render/kind → 26).**
- Mechanism: `ContentResolver::text_of` source line (14) + one `tokenize_item_text(line, Include)` pass + marker-state read + **raw substring scan** for glyphs the lexer skips (Exclude gate `lexer.rs:238-240`; non-ISO skips `:241-262`). Three cases: task lines (accessors), plain bullets with `📅` (visible-and-inert; raw scan only), filtered Checkboxes (gate = marker state, not taskness). Ancestors = list-parent chain only; heading context deferred (no `Heading` node). Markup kind, caps, trust = 26 (isTrusted is client-side, undetectable server-side). Relative-date rendering depends on `today` (18).

**4. Diagnostics — existence + relative severity only; 25 owns taxonomy/publish.**
- Set: A1 unknown symbol (Hint; CLI report primary; client survival → 29), B1 invalid date (Warning), B2 emoji-without-date (Warning, marked items), B3 duplicate slot (**conflicting values only**, Warning; B1 suppresses B3 on same glyph pair), B5 unparseable task field (Warning, task keys only, `as_date().is_none()` precision — no double-fire with 19's structure checks), B6 shorthand-stripped-but-inert on plain lines (**default-off**; vault-calibrated: 9/9 hits intentional), C1 overdue (Warning; needs `today` from 18), C2 due-today (Information), C4 status/date mismatch (Information, O(1)), C5 date-ordering (Information), C6 stale subtask (Information; Done parents only — Cancelled excluded, `fully_complete` counts descendant tasks only), D1 missing-space `[x]task` + D2 rejected `[]`/`[xx]` (Warning; Plain-item raw, first line, with code + link exclusion — detection path must specify raw vs source-line). **B4 (unread emoji) dropped** — definition unrecoverable, readings collide → hover instead.
- Detection caveat: code spans are not on the AST — raw scans need code-exclusion specified. C1/C2 exclude Done **and** Cancelled.
- Every severity reads "23 proposes; 25 decides" (severity framework + Hint=4 verified; per-source override precedent = 20:Q14). Conventions: source `traces-tasks` + per-rule codes; never `MD…` (32's boundary); toggles + `[tasks.diagnostics]` → 31 (deny_unknown_fields means the key cannot land early; interim valve = editor override).
- Delivery: push for open docs via 14's settled 300ms debounce; cross-file panel = **recommendation** of pull `workspace/diagnostic` + `resultId` caching — push-vs-pull is 25's open question, capability gating 29's; workspace-scope default severity → 25.

**5. Symbols — S3 outline with corrected mechanics (content 23, mechanics 27).**
- Shape: hierarchical, Task items, `SymbolKind::Array(18)` (floor-safe; rationale = independent hideability from headings which must be String(15) — a 23↔27 kind dependency), line-granular ranges.
- Nesting: **single pass over `note.lists()` with ancestor stack**; each Task attaches to nearest Task ancestor else root; Plain intermediates traversed not emitted. Never `Note::tasks()` + depth (mis-nests: filter-to-Task detaches tasks from Plain parents — fixture `lists.rs:649`).
- Ranges: leaf `(line,0)..(line+1,0)`; parent `(parent.line,0)..(last_descendant.line+1,0)` (last descendant *start* line +1; no end-line field exists); `selection_range ⊆ range` asserted; equal-line degenerate pinned by test.
- `detail` ≤64 chars from `clean_text()` name + status name + priority + complete/incomplete (bool rollup; **no counts in v1** — v2 priced as one reverse pass) + single Due ISO; `"(untitled)"` fallback for empty clean. detail lost on flat SymbolInformation fallback (29's minimum-viable client). Node cap 1,000 soft / 5,000 hard + post-cap behavior.
- Serve via 14's `ContentResolver` + request-start `Arc<WorkspaceIndex>` snapshot (map.md:22); staleness = 14's 150ms debounce. Range precision (byte spans) = 11/41; registration, code-action plumbing, VS Code built-in-provider coexistence = 27/29 (coexistence currently unowned — must land before any symbol shape ships); heading nesting → 27 blocked on 11's `Heading`.
- Status-cycle code action: 23 defines the invariant *cycle order = config insertion order = completion sort_text order*; 27 owns plumbing; "steps over symbols vs kinds" stays 27's open sub-question.

**6. Workspace/symbol task picker — claimed conditionally (resolves research open call A3).**
- Conditions: (a) `Amends 33` — task rows join the always-resident tier *or* 33 prices vault-wide refill (full reload 15.7–19.6ms baseline / 21.9–28.9ms current; rebuild 406ms@20K); (b) 33 gains a **proposed, unfilled** `workspace/symbol` p95 row derived from `bench_query_tasks_density` with mean→p95 factor stated; (c) cap + ranking assigned to 27/29 (one shared cap across tags 16/headings 27/tasks 23; agree one SymbolKind policy + containerName convention — 16's PROPERTY-vs-STRING unresolved).
- Serve via direct walk over `note.lists()`/`Note::tasks()` + `is_task()`/`clean_text()` — bypasses QueryService only to avoid `QueryRow` materialization and the linear `matched_file_rows` scan (`query/service.rs:305-316`), **not** to build a parallel task model. uri-only locations + `workspaceSymbol/resolve` with **full-Location fallback when `resolveSupport` absent** (unverified client capability → input to 29). Snapshot + overlay via 14.
- Hard requirements, not optimizations: no `$/cancelRequest`/cooperative cancel/`$/progress` (12) → the walk cannot be stopped; cap 128 (16 precedent) + residency defend interactive latency (rust-analyzer 50s incident). No `WorkspaceSymbol.detail`.

**7. Performance (surfaced per map.md:33; `Amends 33`).**
- Resident/line-local features rate ignore-the-budget on measured substrate (prose 0.069ms/10KB; 1,000-item list 1.03ms; 50K-item clone 0.81ms = *floor*, symbol build unmeasured; diagnostics ≪1ms = labeled estimate — no lint bench exists).
- Tension: measured 51.2KB long-line reparse 18.6ms exceeds 33's <10ms single-file target — bench self-flags as unexplained anomaly (`note_parsing.rs:470-476`); root-cause owed to the implementation spec (planning-only). Corrected: bench param is line *length* at fixed bytes, not line count; realistic 4,000-line note ≈2–6ms.
- `[tasks]` config change = full reparse + re-persist + atomic Arc swap ≈ **48/90/466ms @1K/5K/20K** (build 11.3/50.6/406 + persist 36.4/39.6/60.1) — not 13's single-file `RefreshPlan`, content edits unaffected; config detection = 15–50ms background sweep; same-config-generation reads across completion/diagnostics/classification (10's facade + 31's reload). All debounce figures provisional → 25.

**Seams / deferred (prose inputs; `Blocked by: 23` edges added to 24/25/26/27):**
- → 24: dispatch, trigger registration, shared completion-generation function, `[`-context ambiguity at `- [t`.
- → 25: severity taxonomy, publishing, debounce (final tiers), push-vs-pull, day-rollover re-evaluation, workspace-scope defaults, B1/B3 precedence.
- → 26: MarkupContent kind/capability/dispatch, hover mechanics, caps, trust.
- → 27: symbol mechanics, ranges/precision seam, registration, cycle code-action plumbing, coexistence owner, heading nesting (blocked on 11).
- → 29: capability gating (pull diagnostics, resolveSupport fallback, Hint survival), minimum-viable client degradation.
- → 31: `[tasks.diagnostics]` shape + toggles + reload mechanics; record zk per-kind project config vs 20:Q14 tension.
- → 18: `today` primitive (local-vs-UTC, hoisting, day-rollover) — input, C1/C2 blocked on it; candidate generator interface.
- → 32: rumdl non-overlap assertion for task diagnostics; → 34: predicate extraction, `TaskStatusMap::iter()`, visibility widening; → 16: row-shape conventions shared with 23.

Hidden dependencies registered: `byte_to_utf16_cu` absent (11/19 prereq), `today` absent, `Heading` absent (11), `scan_marker_prefix`/`TaskScan` visibility (34), tower-lsp-server not yet in Cargo.toml (09), 18/24/26 open.
