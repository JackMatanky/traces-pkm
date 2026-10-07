# Ticket 24 part-work: codebase review for the unified completion dispatcher

Method: CodeGraph `codegraph_explore` (project is indexed; note the banner that
auto-sync is disabled — all source shown was re-read from disk by the tool and
spot-verified with direct reads/greps) plus direct Read/Grep for tickets,
`Cargo.toml`, and config. Every codebase claim cites `file:line`. Claims taken
from *resolved tickets* (plans, not yet implemented — this is a planning-only
map) are cited to the ticket, never to code. Unverified items are marked
**QUESTION**.

---

## 1. Existing completion-adjacent code

**No LSP-shaped completion logic exists anywhere in `src/`.** Grep for
`tower_lsp|lsp_types|CompletionItem|textDocument` across `src/` returns zero
matches; there is no `src/lsp/` directory (`ls src/`). What does exist:

1. **Shell-completion subcommand** — `src/cli/completions.rs:23-30`
   (`struct Completions`, `--shell` + `--list-templates`). `script()` builds a
   static clap_complete script (`src/cli/completions.rs:78-85`, dep
   `clap_complete` at `Cargo.toml:108`). `--list-templates` prints template
   names for *shell* tab-completion (`src/cli/completions.rs:93-98,112-117`),
   delegating to `TemplateService::list_available` (`src/template/service.rs:83-85`).
   - Reusable at the LSP boundary: **partially.** `TemplateService::list_available(&Config) -> Vec<String>`
     is a plain synchronous function and is exactly the candidate source for
     template-name completion (ticket 22 already plans an overlay-aware seam on
     `find`/`list_available`, ticket 22 Q20(b)). The clap script generation is
     not reusable.

2. **Interactive CLI prompts** — `src/dialog/terminal.rs:54` (`inquire::Text`),
   `:86` (`inquire::Select::new(label, items…)`), `:103` (`inquire::MultiSelect`),
   with the `fuzzy` feature enabled (`Cargo.toml:112-116`). These are blocking
   terminal TUI prompts, **not reusable** in an LSP process (they require a TTY
   and drive a human). Their fuzziness lives inside `inquire`.

3. **Suggestion/matching primitives (reusable)** — the codebase already has a
   small "suggest closest match" vocabulary used by diagnostics, not completion:
   - `closest_match` (Levenshtein, accept ≤ ceil(len/2) threshold, min 1) —
     `src/strsim.rs:10-21`, backed by the `strsim` crate (`Cargo.toml:157`).
   - Used by `Schema::suggest_field` (`src/schema/model.rs:129-146`; exact
     `FieldKey::is_match` canonical match first, then
     `closest_field_name` → `closest_match`) and by `FieldPath::closest_accessor`
     for query accessors (`src/query/grammar/field.rs:141-146`).
   - `FieldKey::is_match` canonical (case/hyphen-normalized) equality —
     `src/field.rs:318-320`. This is the dedup/normalization primitive ticket
     19's field-key layers rely on.
   - All synchronous, all in-memory, all O(candidates × input) — cheap enough
     to run per-resolve.

**Conclusion**: nothing completion-shaped is reusable, but three primitives are
(`TemplateService::list_available`, `closest_match`, `FieldKey::is_match`).

## 2. Note AST + spans (tickets 11/19 resolved decisions vs. today's code)

**Today's AST carries line numbers or nothing — no byte spans on semantic nodes:**

| Node | Definition | Position data today |
|---|---|---|
| `Note` | `src/note/model.rs:23` (fields assembled in `new` :42-71: `path`, `frontmatter`, `lists`, `headings`, `outlinks`, `inline_fields: IndexMap<FieldKey, Vec<NoteFieldValue>>` :47, `tags`) | none per-node |
| `Link` | `src/note/links.rs:27-32` (`target`, `text`, `kind`, `embedded`) | **none** |
| `Tag` | `src/tag.rs:28` (`Tag(Box<str>)` newtype) | **none** |
| `Heading` | `src/note/heading.rs:9`; `Heading::new` :19 | 1-indexed `SourceLine` only (`heading.rs:44`) |
| Inline fields | `src/note/model.rs:47` IndexMap | **none** |
| `Frontmatter` | `src/note/metadata.rs:96-98` (`IndexMap<FieldKey, NoteFieldValue>`) | **none**; raw YAML parsed at `metadata.rs:56-75` (`yaml::parse`), spans discarded; `src/yaml.rs` has zero span references (grep) |
| `ListItem` | `src/note/lists.rs:201` (`line`), `:209` (`parent`) | 1-indexed `SourceLine` |

**Span machinery exists but is used elsewhere, not on these nodes:**

- `BytePos` (u32 newtype) `src/position.rs:188`; `ByteSpan` `src/position.rs:77`;
  `Spanned<T>` (value + `ByteSpan`, `span()` `:386`, `span_usize()` `:393`,
  `from_usize_range` `:372`) `src/position.rs:353-404`; re-exported at
  `src/lib.rs:132`.
- `LineIndex` `src/position.rs:32` with O(log n) `line_at` `:56` (doc :29-30).
- `Spanned` is consumed today by the **query grammar** (`src/lexer.rs:20-21`
  `SpannedTokenStream<T>`; `src/query/grammar/source.rs:117`,
  `filter.rs:47`, `expr.rs:90`) and the **task-item lexer**
  (`src/note/parser/lexer.rs:43-49`, `src/note/parser/task.rs:23`).
- The Markdown parser **already receives byte spans and throws them away**:
  `parse_markdown` iterates `into_offset_iter()` and passes
  `ByteSpan::from(range)` per event (`src/note/parser.rs:108-113`), but
  `handle_event` (`:241-258`) only uses `span.start()` (list-nesting line
  tracking via `handle_start_tag` `:247,264-268`) and `span.to_range()` for
  footnote raw text (`:254`). Link/tag/inline-field spans are never stored.
  Headings are collected in a **second parser pass**
  (`collect_headings` `:118-157`, `HEADING_OPTIONS`, line-only at `:139`).

**`byte_to_utf16_cu` does not exist** — grep for `utf16` over `src/` returns
zero matches. It was *decided* in ticket 11 §4 (add to `LineIndex`, walks chars
summing `len_utf16`) and §5 (file buffers use `LineIndex` + `byte_to_utf16_cu`;
live buffers use ropey's native API) — not implemented.

**Resolved decisions the dispatcher will lean on** (all *planned*, ticket 11):
- §1 (ticket 11:33): add `Range<BytePos>` to `Link`, `Tag`, inline-field
  entries, new `Heading` node; capture from `into_offset_iter()` (already in
  hand, see above) and logos token spans; parallel LSP-only span index
  explicitly rejected.
- §6 (ticket 11:60-62): frontmatter byte ranges via `noyalib` `Spanned<T>`,
  landing with ticket 19 — `noyalib` is already a dependency
  (`Cargo.toml:131-139`) but `src/yaml.rs` doesn't use span extraction today.
- §7 (ticket 11:64-66): stay on pulldown-cmark, **full re-parse per edit**;
  §8 (ticket 11:68-70): spans transient, never persisted in redb;
  §9 (ticket 11:72-74): `ropey::Rope` for live buffers only.

**Cursor-classification implication**: until ticket 11's spans land, a
dispatcher can only classify cursor position by lexical re-scan of the current
buffer text (frontmatter delimiter scan, line-shape predicates) — which is in
fact what tickets 19 and 23 specify for their position gates (see §7's
inventory). Once spans land, span containment tests become available.

## 3. Buffer/overlay model (ticket 14) — planned, not present

Grep for `ContentResolver|DocumentStore|ropey` across `*.rs` → **zero matches**.
None of ticket 14's machinery exists yet. What exists to build on:

- `WorkspaceIndex { entries: Box<[FileEntry]> }` — `src/index/entry.rs:20-22`;
  `entries()` `:106`; `entry_at(RowIndex)` `:122` (O(1) by position).
- `FileEntry { file: FileMeta, note: Option<Box<Note>>, inlinks: Box<[PathBuf]> }`
  — `src/index/entry.rs:134-138`; `note()` `:159`; `inlinks()` `:166`. **All
  parsed Note ASTs are resident in memory** today (confirmed by ticket 33's
  amendment: "Today's `WorkspaceIndex::assemble` keeps all ASTs resident",
  ticket 33:116).
- `InlinkMap(HashMap<PathBuf, Box<[PathBuf]>>)` — `src/index/inlinks.rs:58`;
  incremental `patch_and_diff` `:216-247`.
- `LinkResolver<'a>` — `src/index/inlinks.rs:318-321`: basename → paths
  `FxHashMap` built in one O(n) pass (`:324-333`); `resolve()` `:368-380`
  tries exact path → path-without-`.md` → nearest basename (Obsidian-style).
  Currently `pub(super)` to `src/index` (ticket 14:29 already notes the API
  widening this needs).
- Note parse entry point for overlay content: `parse_markdown(&MarkdownParserInput)`
  — `src/note/parser.rs:105`; input type `src/note/parser/input.rs:24,39`.
  Synchronous, no I/O.
- Query entry takes `&Arc<WorkspaceIndex>` — `src/query/service.rs:103-107`.

**Resolved ticket-14 decisions the dispatcher must honor**: transient
replacement map `RwLock<HashMap<Url, BufferState { content: String, version: i32 }>>`
with **no cached parsed note** — `note_of(uri)` parses overlay content
*on demand* (`Cow<Note>`, ticket 14:23-27); 150ms debounced full re-parse
(ticket 14:25); `ContentResolver::text_of`/`note_of` is the single content
facade (ticket 14:27). **Consequence for completion**: serving the currently
edited file pays a fresh `parse_markdown` per request unless the dispatcher
caches (QUESTION: no cache decision exists for completion's own context-detect
parse; ticket 14 deliberately chose on-demand parsing over a cached
`parsed_note`).

## 4. Services a completion dispatcher would call

All are **synchronous**; ticket 09 (resolved) keeps the core sync and confines
tokio to the transport (ticket 09:22: "Every Index/Query/Schema/Template/
parsing call stays synchronous, invoked from async handlers via blocking-task
dispatch"). So: the dispatcher runs sync inside a blocking task.

| Service | Entry points | Sync? | Cost shape | What completion gets |
|---|---|---|---|---|
| `QueryService` | `src/query/service.rs:58`; `run` `:103-112` | sync | **`rows_for` → `matched_file_rows` is a linear filter over every index entry** (`service.rs:305-316`, `source.is_match` per row) — O(vault) per call | `QuerySet` (`src/query/results.rs:451`), lazy materialized once behind `OnceLock` (`:470-478`) — good reuse shape (build plan once, read many) |
| `run_from_store` | `service.rs:125-165` | sync | redb batch reads + `rayon::join` + temp index assembly — **disk I/O, never per-keystroke** | cold-start queries only |
| `SchemaService` | `src/schema/service.rs:36`; `new(directory)` `:62` (loads `.traces/schemas/*.toml`, `service.rs:3`) | sync, in-memory after load | O(1) `get(name)` `:91`; small-vec `children_of` `:101`, `descendants_of` `:136`, `matches` `:165` | `Schema::fields()` (`src/schema/model.rs:71`), `Schema::field(name)` `:79`, `Schema::suggest_field` `:129-146`. **Gaps: no `suggest_class()` and no "list all schemas" method** (grep of `schema/service.rs` public fns) — ticket 21 lists `SchemaService::suggest_class()` (~15 LOC) as a pre-condition (ticket 21, "Pre-conditions"); ticket 19's "global schema fields" layer has **no existing enumeration API** (QUESTION: who adds it) |
| `IndexStore` (redb) | `src/index/store.rs:152` (`open`) | sync, **disk-backed** | `paths_with_tag` `:262-276` (multimap point lookup, lowercased), `paths_with_file_class` `:285-292`, `paths_in_folder` `:301-310` (walks `FILES` table via `collect_folder_paths`); batch reads `:187,:202,:221` | per-tag/class/folder path sets. **`unique_tags()` / `unique_file_classes()` do not exist** (no `fn unique_*` anywhere in `src/`; map.md:84 lists both as *prerequisites* for ticket 21 — `unique_tags() -> Vec<(Tag,u32)>` ~30 LOC, `unique_file_classes()` ~20 LOC). Enumerating distinct values today would need a full multimap scan — `collect_multimap_values` `:1219-1231` is per-key only |
| Template analysis | **`src/template/analysis.rs` does not exist** (`ls src/template`: engine/, loader, service, writer, path, error) | — | — | Ticket 22 decided placement (ticket 22:32) and that no static analysis exists today: templates execute side-effectful `ui.*` mid-render, so completion must go through minijinja `Environment::parse` AST inspection (ticket 22:9,12). Engine holds one shared `debug` env and re-parses per render (ticket 22:9 citing `src/template/engine.rs:71,117,172-175`). Also carries an explicit **degrade/three-state** contract ("data-source guarantee… third explicit state… threaded to 24", ticket 22 Q15c) that the dispatcher must surface |
| `WorkspaceIndex` | `src/index/entry.rs:20` | sync, resident | O(1) `entry_at`; O(n) path/basename scans | file paths/stems for wikilink completion; per-note `Note` for headings/tags/fields of one file. No inverted indexes exist yet (ticket 33's `wiki_link_index`/`tag_index`/`file_class_index` at ticket 33:57-64 are *design*, not code) |
| `TaskStatusMap` | `src/task.rs:117`; `insert` `:199`; `by_symbol`/`by_name`/`by_type` (impl at `:123+`) | sync, from config | O(1) lookups | checkbox-symbol candidates for ticket 23's `- [` completion. **No `iter()` method exists** — ticket 23:24 explicitly says "add `iter()`, ordering = config insertion order = `sort_text`" |
| `TemplateService::list_available` | `src/template/service.rs:83-85` | sync | config + filesystem listing of template dirs | template-name completion candidates |

## 5. Config surfaces completion must honor (`src/config/`)

- **`[frontmatter]`** — `FrontmatterConfig` `src/config/model.rs:399`;
  `tags_name()` `:425` (default `"tags"`, `:45-46`), `title_name` `:411`,
  `aliases_name` `:418`, `date_created`/`date_modified` `:432/:439`. Tag
  completion inside frontmatter and the frontmatter tags key must read this.
- **`[tasks]`** — `TaskConfig` `src/config/model.rs:565`; `statuses()` `:578`
  returns the `TaskStatusMap`; statuses are built by iterating
  `raw.statuses` in file order (`:646-667`), and ticket 23 fixes the invariant
  **config insertion order = cycle order = completion `sort_text` order**
  (ticket 23:24, 23:47). Tag filters (`:584`) gate task classification, hence
  which `- [` positions are task-completion contexts.
- **`[schemas]`** — `SchemasConfig` `src/config/model.rs:297`;
  `class_field_name` default `"class"` (`:36`); registry dir default
  `.traces/schemas/` (`:31`). The class field name determines which frontmatter
  key scopes schema-scoped field completion (ticket 19/20 boundary).
- **No completion ordering/cap config exists** — grep for a `limit`-style key
  in `src/config/` finds none (only "delimiter" substring hits). Ordering is
  decided per-ticket instead: sortText layer prefixes (ticket 19:48),
  config-insertion order (ticket 23). **QUESTION**: whether ticket 24 wants a
  max-items config; nothing in the codebase suggests one today.
- Config load path: `ConfigService` (`src/config/service.rs:149`) with trust
  verification (`verify_trust` `:265`) — the LSP's config access must go
  through whatever ticket 31 decides for untrusted roots; CLI precedent is
  `load_config(service)` (`src/cli/task.rs:192`).

## 6. LSP code today + `Cargo.toml`

- **No LSP module, no LSP dependencies.** `[dependencies]` (`Cargo.toml:103-163`)
  contains **no** tokio, tower-lsp(-server), lsp-types, lsp-server,
  fuzzy-matcher, ropey, or camino. Relevant existing deps for this ticket:
  - `inquire … features=["fuzzy"]` `Cargo.toml:112-116` (CLI TUI only)
  - `logos` `:117` (lexer — query-DSL token classification for completion, ticket 21)
  - `minijinja` `:119-130` (templates; its `Environment::parse` is the planned analysis surface)
  - `noyalib` `:131-139` (YAML; `Spanned` frontmatter spans decided in ticket 11 §6)
  - `pulldown-cmark` `:149`, `rayon` `:150`, `redb` `:151`
  - `strsim` `:157` (closest_match), `rustc-hash` `:153` (FxHashMap), `walkdir` `:163`
  - **Planned but not yet added**: `fuzzy-matcher` (wikilink completion scoring
    with highlight indices) and `camino` — ticket 15:64, ticket 15:233.
- Ticket 09 (resolved) decides the future: `tower-lsp-server` 0.23.0 + tokio
  **transport-loop-only**; core services stay synchronous, called from async
  handlers via blocking dispatch (ticket 09:20, 09:22).
- `docs/refs/lsp_spec.md` exists (cited by ticket 24:12 for the
  Completion Request section).

## 7. Performance grounding (ticket 33: completion <20ms p95)

Budget: **completion <20ms p95, stretch <10ms**, strategy "wiki-link + heading
completion from inverted index; **no linear scan**" (ticket 33:50); inverted
index design (ticket 33:57-64) is *specified but not implemented* — verified:
`WorkspaceIndex` holds only `entries` (`src/index/entry.rs:21`), no tag/class
maps; `unique_*` absent (§4).

Where candidate generation can blow the budget, from what exists today:

1. **`QueryService` linear scan** — `matched_file_rows`
   (`src/query/service.rs:305-316`) filters every entry per call. If the
   dispatcher derives candidates via a query, it pays O(vault) per keystroke.
   (ticket 33:9 already flags this scan as the open problem.)
2. **Unique tag/class enumeration** — no API; doing it on request = full redb
   multimap scan (`store.rs:1219-1231` is per-key; nothing collects distinct
   keys). Must be prebuilt (ticket 21's `unique_tags()`/`unique_file_classes()`,
   ticket 33's `tag_index`).
3. **Overlay note parse** — ticket 14:23 deliberately has *no* cached
   `parsed_note`; `note_of()` re-parses the current buffer per call. Full
   re-parse of the edited note is on every completion request's hot path unless
   the dispatcher adds its own memoization (QUESTION: not decided anywhere).
   Baseline: pulldown-cmark ≈0.20ms/48KB (ticket 33:39) — fine — **but** the
   `note_parsing` bench records an anomaly: 18.6ms on a 51.2KB fixture with
   4,000-*character* lines, above the <10ms single-file target, root cause
   still owed (ticket 33:118, `benches/note_parsing.rs:470-476`). Pathological
   long-line notes can therefore eat the entire completion budget in one
   context-detect parse.
4. **`run_from_store`** (`service.rs:125-165`) — redb I/O + temp index build;
   far too expensive for per-keystroke; cold-path only.
5. **`paths_in_folder`** (`store.rs:301-310`) — walks the `FILES` table with a
   prefix filter rather than an inverted folder index; folder-path completion
   (query source expressions) at 20K files is a scan of the files table per
   request (though it is in-redb sorted-key order; actual cost **QUESTION** —
   not benchmarked here).
6. **Cheap/safe**: `SchemaService` lookups are in-memory hash/indexmap gets —
   "hash-map lookup + type check — O(1), negligible" (ticket 33:90);
   `LinkResolver` build is O(n) once, then O(1) lookups (`inlinks.rs:324-333`);
   `QuerySet`'s `OnceLock` cache means a plan runs at most once per set
   (`results.rs:467-478`).

---

## Integration points & constraints for a unified completion dispatcher

**What exists to reuse (verified in code today)**
- Position/vocabulary layer: `BytePos`/`ByteSpan`/`Spanned`/`LineIndex`
  (`src/position.rs:77,188,353,32`) and the `SpannedTokenStream` lexer wrapper
  (`src/lexer.rs:20-21`) — already proven in the query grammar and task lexer.
- Parser byte spans *in hand but discarded* at `src/note/parser.rs:108-113,241-258`
  — extending `Link`/`Tag`/inline-fields per ticket 11 §1 is a small plumbing
  job, not new infrastructure.
- Candidate/score primitives: `closest_match` (`src/strsim.rs:10`),
  `FieldKey::is_match` (`src/field.rs:318`), `Schema::suggest_field`
  (`src/schema/model.rs:129`), `LinkResolver` basename index
  (`src/index/inlinks.rs:318`), `TemplateService::list_available`
  (`src/template/service.rs:83`).
- `QuerySet` lazy-plan pattern (`src/query/results.rs:451,470-478`) as the
  template for "build cheap, materialize once".

**What each of the six completion contexts needs, and where it comes from**

| Context | Position predicate | Candidate source | Missing prerequisite |
|---|---|---|---|
| Wikilink target (15) | span-of-`[[…]]` (post-ticket 11); lexical `[[` scan today | `WorkspaceIndex::entries()` + basename index (LinkResolver-style) | `fuzzy-matcher` dep (ticket 15:64); heading slugs for `[[#` (ticket 15:239) |
| Tag (16) | `#` + syntax guards incl. heading/link-anchor exclusions (ticket 16:32-37) | unique tag list | **`IndexStore::unique_tags()`** (map.md:84); hierarchical children filtering over it |
| Frontmatter / inline field (19) | frontmatter region scan for `:` after YAML-key regex; `[` for inline (ticket 19:81-84); `CodeRegion` exclusion first | layer 1: note's fileClass `Schema::fields()` (exists); layer 2: all global schema fields; layer 3: vault-wide inferred keys, sortText-prefixed by layer (ticket 19:46-50) | layer 2: SchemaService list-all API (gap, §4); layer 3: `VaultFieldIndex` (doesn't exist); frontmatter spans via noyalib `Spanned` (ticket 11 §6) |
| Query DSL in template (21) | depth counter over `{{`/`}}` (ticket 21/map.md:84); logos token classification of text-before-cursor | tags after `#`, classes after `@`, `Schema::fields()` for LHS, operators/select options | **`unique_tags()`, `unique_file_classes()`, `SchemaService::suggest_class()`** (ticket 21 pre-conditions) |
| Template helper namespace (22) | inside `{{ }}`, after `.` in `ns.member` | probe-derived namespace/function metadata table | **`src/template/analysis.rs`** + degrade-state plumbing (ticket 22:32, Q15c) |
| Checkbox status (23, also blocks 24) | buffer-local O(line) predicate gated on `scan_marker_prefix == Incomplete` (`src/note/parser/marker.rs:86,73`) | `TaskStatusMap` in config-insertion order | `TaskStatusMap::iter()` (doesn't exist; ticket 23:24); visibility widening of `scan_marker_prefix` rides ticket 34 (ticket 23:24) |

**Sync/async shape**: all candidate sources are synchronous; per ticket 09:22
the async handler dispatches the whole `textDocument/completion` computation as
one blocking task. No service returns futures; nothing in the dispatcher needs
`await` except cancellation (ticket 12 owns checkpoints).

**Trigger-character inventory already proposed *into* ticket 24** (this ticket
owns the final set): `#` (ticket 16:32; ticket 21:49), `@` (ticket 21:49),
`[` (ticket 19:81; ticket 23:24 — with `- [` ambiguity explicitly delegated
to 24, ticket 23:60), `:` (ticket 19:81, with frontmatter/`::` guards),
explicit rejections: `(` dropped as standalone trigger (ticket 19:84), no
space trigger (ticket 23:24), phase-2 `(` and `"` (ticket 21). **Not yet
claimed by anyone**: `.` (template `ns.` member completion — ticket 22 does
not register it; QUESTION) and `/` (hierarchical tag children, ticket 16:11 —
no trigger registration stated; QUESTION).

**rumdl collision check**: ticket 24:12 asserts rumdl registers `(`, `#`, `/`,
`.`, `-`. The repo's rumdl research (`research/tool-rumdl-boundary.md`) documents
`enableLinkCompletions`/`enableLinkNavigation`/`enableSymbols` and the
recommended three-setting disable (lines 21, 36, 42) but contains **no
trigger-character enumeration** (grep for "trigger" → zero hits). **QUESTION:
the rumdl trigger set must be re-verified against rumdl's source/docs before
ticket 24 relies on it.** What *is* verified: rumdl never completes wikilinks
(tool-rumdl-boundary.md:47) and maintains its own parallel WorkspaceIndex for
standard-link completion (tool-rumdl-boundary.md:56).

**`CompletionItem` resolve + snippets**: no other ticket settles either.
`completionItem/resolve` is explicitly deferred *from* ticket 21 into 24's
scope (ticket 21:63-67); `insertTextFormat: Snippet` appears only in ticket
24:14 itself — no per-feature ticket has claimed snippet-shaped insertions
(grep across all issues). Both are genuinely open decisions for this ticket.

**Cost hot-spots to design around (summary)**: (a) any per-request vault-wide
scan — `matched_file_rows` (`service.rs:305-316`) or redb multimap enumeration
(`store.rs:1219`) — must be replaced by the ticket-33 inverted indexes /
ticket-21 `unique_*` primitives; (b) per-request overlay re-parse of the edited
note (ticket 14:23) with the documented long-line anomaly (ticket 33:118) is
the one unavoidable per-keystroke cost unless memoized; (c) everything else
(schema, basename, status-map, template-list) is resident hash-map lookups
comfortably inside the <20ms budget.
