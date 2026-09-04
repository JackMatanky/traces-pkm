# Architecture Review — LISTS Persistence and Query Row Model

Status: findings — pre-implementation triage input for issue 08+

**Date**: 2026-09-04
**Scope**: `note::ListRecord`/`LISTS` table (issue 07, unmerged branch
`task-system/07-note-api-and-lists-persistence` @ `f644697`), `query::QueryRow`/`RowKind`/`TaskField`
(pre-issue-08 state on `main` @ `8dabc16`), ADR 0005, `spec.md`, tickets 08–11,
domain `CONTEXT.md` files, and the Obsidian Dataview list/task data model
(`docs/digests/obsidian_blacksmithgu-obsidian-dataview-digest.txt`).
**Method**: source reading across `src/index/`, `src/query/`, `src/note/`,
`src/template/engine/query.rs`, `docs/adr/`, `CONTEXT-MAP.md` and per-module
`CONTEXT.md`; one empirical measurement (scratch test, discarded, see Finding 3).
**Not in scope**: no production code was changed. This is a design report only.

---

## Executive summary, ranked by severity

| # | Finding | Severity |
|---|---|---|
| 1 | `LISTS` table has **zero production consumers** — `query_tasks` still walks the live in-memory `Note` tree; `read_lists*` is only called by its own test | **Critical** — issue 07 shipped a persistence layer nothing reads |
| 2 | `LISTS` rows **empirically duplicate entire descendant subtrees** — confirmed O(depth²)-shaped storage growth | **Critical** — a real bug, not a style nit, hiding inside "done" work |
| 3 | `ListRecord` should be `ListEntry` in `src/index/entry.rs`, not `note/lists.rs` | **High** — user's own finding, confirmed by the module's own doc comments and by precedent (`FileEntry`/`NoteEntry`) |
| 4 | `TaskRow`/`RowKind::Task` should be a `ListRow` with an optional task overlay, mirroring Dataview's `SListItem = SListEntry \| STask` | **High** — user's own finding; blocks a real query mode (`Note.list_items()`-equivalent) that currently cannot exist |
| 5 | No ADR documents the `LISTS` table; ADR 0005's accepted 2-table schema and the original grilling-session design both put `tasks`/`lists` **inside** `note_metadata`, never as a 3rd table | **High** — governance gap, not just naming |
| 6 | Three different names for the same query-row concept across ADR (`IndexRecord`/`QueryOutcome`), code (`QueryRow`/`QuerySet`), and ticket 08 (`QueryRecord`, which doesn't exist) | **High** — ticket 08 as literally written cannot be implemented; it names a type that isn't in the codebase |
| 7 | Domain glossary (`CONTEXT.md`) was never updated despite the spec's own explicit mandate to do so | **Medium** — spec self-violation, easy to fix, but signals process drift across 7 already-merged/committed issues |
| 8 | `ListRecord.path: String` is speculative complexity — a per-row duplicated join key with no current reader | **Medium** — tied to Finding 1; the *pattern* (path-keyed re-join) is fine, but nothing exercises it |
| 9 | `"list"` is a four-way naming collision across CLI, template filter, domain model, and the proposed new type | **Medium** — will get worse, not better, if not addressed before `ListRow`/`Lists` mode ship |
| 10 | `ListRecord` is unconditionally `pub`, but its only producer (`IndexerService::read_lists`) is `pub(crate)`/test-gated | **Low** — inconsistent but currently harmless (nothing external can reach it either way) |
| 11 | Ticket 08's "backward compatibility" language contradicts spec's own "Out of Scope: backward compatibility... project has not shipped" | **Low** — wording bug, one-line fix |

**Bottom line**: issue 07 is not actually blocking issue 08 the way the ticket
graph assumes. Ticket 08 as currently drafted ("Enrich `QueryRecord`...")
would either (a) fail immediately because `QueryRecord` doesn't exist, or (b)
get "fixed" by someone silently renaming it to `QueryRow` and bolting more
fields onto `TaskRow`, which **cements** the `TaskRow`-not-`ListRow` mistake
and the LISTS-table dead-code problem for another release cycle. Recommend
amending issue 07 (still unmerged, zero external consumers, 38 occurrences
across 6 files — cheap now, expensive later) before starting 08.

---

## Finding 1 — `LISTS` table has zero production consumers

`src/query/service.rs` builds task-level query rows like this:

```rust
fn task_rows(
    &self,
    index: &Arc<FileIndex>,
    source: &SourceSelector,
) -> Vec<QueryRow> {
    let mut out = Vec::new();
    for base in self.matched_file_rows(index, source) {
        let Some(note) = base.note() else { continue };
        for item in note.tasks() {                       // <-- live tree walk
            out.push(base.clone().with_task_item(item));
        }
    }
    out
}
```

`note.tasks()` walks the in-memory `Note.lists: Vec<List>` tree (a
`ListItemIter` over data that was deserialized from the `NOTES` table when
`FileIndex` was built or loaded). It never touches `IndexStore::read_lists`,
`read_lists_for_path`, or `read_all_lists`.

Grep confirms: `read_lists`, `read_all_lists`, and `read_lists_for_path` have
**exactly one call site each**, and it's their own test
(`tests/integration/index_persistence_roundtrip.rs`). No CLI command, query
path, or template namespace calls them. `IndexerService` itself is
`pub(crate)` in non-test builds (`src/lib.rs:101-106`), so in a real release
build there is no path to `read_lists()` from outside the crate either.

Why this matters: user story #41 in the spec ("I want task rows persisted in
the FileIndex, so that repeated queries do not reparse every Note
unnecessarily") is **already satisfied by the `NOTES` table alone** — `Note`
derives `Serialize`/`Deserialize` and carries `lists: Vec<List>` directly, so
loading `FileIndex` from disk already reconstructs the full list/task tree
without reparsing markdown. `LISTS` doesn't make anything faster today; it's
a second, disconnected persistence of the same data with no reader.

This isn't hypothetical dead code that "might get used" — it's a completed,
tested, ~150-line subsystem (table definition, key encoding, `ListRecordRef`
borrowed-serialization optimization, incremental delete/upsert wiring,
`should_rebuild` schema-drift detection) built entirely for a consumer that
was never wired up, and ticket 08 as drafted does not wire it up either (see
Finding 6).

## Finding 2 — Empirically confirmed quadratic storage blowup

`ListItem` is:

```rust
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct ListItem {
    text: ListText,
    kind: ListItemType,
    children: Vec<List>,        // <-- no #[serde(skip)]
    fields: IndexMap<FieldKey, Vec<NoteFieldValue>>,
    position: ListItemPosition,
}
```

`write_lists_for_note` serializes one `LISTS` row per line via
`ListRecordRef<'a> { path: &'a str, item: &'a ListItem }`, iterating
`note.list_items()` — a depth-first walk that yields **every** item,
including every descendant. Because `ListItem.children` is not skipped
during serialization, postcard-encoding an ancestor's row embeds the full
serialized subtree of everything nested beneath it. Every descendant then
*also* gets its own row, which *again* embeds everything beneath **it**. The
same subtree bytes are duplicated once per ancestor.

I verified this directly (scratch `#[cfg(test)]` unit test added to
`src/index/store.rs`, run, and discarded — no code left behind) against a
markdown file with 6 list items nested one inside the next:

```text
LISTS row byte sizes by source line (0-indexed depth order):
  line 1: 348 bytes   (root — embeds all 5 descendants)
  line 2: 292 bytes
  line 3: 235 bytes
  line 4: 178 bytes
  line 5: 121 bytes
  line 6: 64 bytes    (leaf — embeds nothing)
root_size=348 leaf_size=64 ratio=5.4x
```

Total: 1,238 bytes for 6 items whose "flat" cost (each row storing only its
own line) would be ≈384 bytes — **3.2x overhead at depth 6**, and this ratio
grows with nesting depth: a chain of *N* items costs `O(N²)` bytes overall,
not `O(N)`. Real project/task hierarchies (nested subtasks under a project
under an area) commonly run 4-6 levels deep; a 20-item nested checklist would
be markedly worse than this 6-item measurement suggests.

This is a genuine defect, not a style preference: a table whose own doc
comment says "keyed by `(path, line)`" implies one row = one line's worth of
data, but the value type silently carries the entire remaining subtree. If
`LISTS` is ever wired up to a real reader (Finding 1), this bug ships with
it. If it's fixed independently first, whoever fixes it needs to know the
row's *value* type must not be `&ListItem` — it needs a children-stripped
projection (a leaf-only struct, or `ListItem` split into "own data" vs
"nested lists" so the flat row only serializes the former).

## Finding 3 — `ListRecord` belongs in `index/entry.rs` as `ListEntry`

Confirmed as stated by the user. `ListRecord`'s own doc comment says:

> "A **persisted record** of a single list item and its source note path."

That is exactly the job description of `src/index/entry.rs`, whose module
doc says:

> "`FileIndex` and its constituent `FileEntry` rows."

`FileEntry` (a file's metadata + parsed `Note` + inlinks) and `NoteEntry`
(the boxed `Note` + inlinks payload) both live there — that module is
**already** the established seam for "a persisted row shape derived from
indexed data." `ListRecord` is structurally identical in role (a persisted,
flattened, per-key row over indexed data) but was placed in `note/lists.rs`,
which is the *parsing/domain* module, not the *persistence* module. Every
other persisted row type in this codebase (`FileEntry`, `NoteEntry`) is named
`*Entry` and lives in `index/`; `ListRecord` breaks both the naming
convention and the module placement convention.

Rename `ListRecord` → `ListEntry`, move it to `src/index/entry.rs`. Zero
migration cost: the branch is unmerged, `ListRecord` isn't referenced outside
this repo, and it appears in exactly 6 files / 38 occurrences (mechanical
`lsp rename` + move).

While relocating: update `index/CONTEXT.md`'s "Indexed Data" section, which
currently documents `File Base`, `Note`, `Inlink`, and `Incremental Delta`
but has no entry at all for the `LISTS` table or its row type — despite
`store.rs`'s own module doc listing it as one of the four things `IndexStore`
persists (`FileBase`, `Note`, `ListRecord`, inlink records). The domain
glossary is out of sync with the code it's supposed to document (see also
Finding 7).

## Finding 4 — `TaskRow` should be a `ListRow` with an optional task overlay

This is the user's core architectural question, and Dataview's own data
model (which the spec was explicitly asked to learn from) answers it
directly. From `docs/docs/annotation/metadata-tasks.md` and
`src/data-model/serialized/markdown.ts` in the digest:

```typescript
export type SListItem = SListEntry | STask;

export interface SListItemBase {
    symbol, link, section, path, line, lineCount, position, list,
    blockId?, parent?, children: SListItem[], outlinks, text, visual?,
    annotated?, tags: string[], ...
}

export interface SListEntry extends SListItemBase { task: false }

export interface STask extends SListItemBase {
    task: true;
    status, checked, completed, fullyCompleted,
    created?, due?, completion?, start?, scheduled?;
}
```

Dataview has **one list-item shape** with universal fields
(`SListItemBase` — text, line, position, parent, children, tags, path) and a
task-only *extension* layered on top (`STask`'s extra fields), discriminated
by a `task: boolean` tag. Both variants are queryable as "list items";
`file.lists` returns all of them, `file.tasks` returns only the `task: true`
ones, and a `TASK` query flattens *either* to top-level rows with inherited
page fields ("Tasks inherit all fields from their parent page").

Compare to the current (pre-issue-08) `traces-pkm` model:

```rust
enum RowKind { Page, Task(TaskRow) }
struct TaskRow { status: TaskStatus, text: String }
```

There is no slot for a non-task list item at the query layer at all.
`QueryMode` (`src/query/builder.rs`) has exactly `Pages | Tasks` — confirmed
by `src/query/CONTEXT.md`'s own glossary: *"Query Mode: the row evaluation
granularity of a query: `Pages` (one row per Note) or `Tasks` (one row per
task checklist item)."* This is baked into the domain vocabulary, not just
an implementation detail.

This is a real capability gap, not a naming quibble: `Note.list_items()`
(the *unfiltered* iterator, added in issue 07 specifically "so that advanced
code can inspect plain bullets and non-task checkboxes" — spec
user story 40) and the entire `LISTS` table's scope (it persists **every**
list item,
not just tasks — confirmed by `write_lists_for_note` iterating
`note.list_items()`, not `note.tasks()`) both already model "list item,
which may or may not be a task." Only the query layer refuses to represent
that. If a future ticket wants "show me all open (non-task) checklist items
across my vault" — a completely reasonable PKM query, and exactly what
Dataview's `LIST FROM file.lists` answers — there is currently no row shape
to hold the result. `TaskRow` structurally *cannot* represent a plain bullet,
because it assumes task-ness (it stores `status: TaskStatus` unconditionally,
not `Option<TaskListItem>`).

**Recommended shape** (design only, not implementing):

```rust
enum RowKind { Page, List(ListRow) }

struct ListRow {
    text: String,                    // clean text, universal to every item kind
    line: SourceLine,
    depth: u8,
    parent: Option<SourceLine>,
    tags: Vec<Tag>,                  // item-level tags, universal
    task: Option<TaskFields>,        // None for Plain/Checkbox, Some for Task
}

struct TaskFields {
    status: TaskStatus,
    priority: Option<TaskPriority>,
    dates: TaskDates,
    fully_complete: bool,
}
```

And `QueryMode::Lists` (unfiltered, mirrors `Note.list_items()`) alongside
the now-more-specific `QueryMode::Tasks` (filtered, mirrors `Note.tasks()`)
— exactly the unfiltered/filtered pairing issue 07 already established for
`ListItemIter`, reused at the query layer instead of invented fresh.

Why not just add a second, fully separate row/set family for lists (keep
`QueryRow`/`TaskRow` untouched, add `ListQueryRow`/`ListQuerySet` alongside)?
Because `QuerySet`'s entire value — the CTE-style transform pipeline
(`filter`/`sort`/`limit`/`group_by`/`flatten`) and terminal renderers
(`table`/`list`/`task_list`/`count`), all generic over one row type — is
exactly the kind of deep, reusable machinery the deletion test says is
earning its keep. Duplicating that whole pipeline for a second row family to
avoid widening one enum is the shallow-module mistake: two near-identical
large interfaces instead of one, deepened. `task.<field>` field-path syntax
(`FieldPath::Task(TaskField)`) doesn't need to change either — it still
resolves through `RowKind::List(row) => row.task.as_ref()?...`, so this
reshaping is compatible with tickets 08/09/10's already-drafted field-path
checklists; it changes the row's Rust shape, not the query-surface
vocabulary.

## Finding 5 — No ADR covers `LISTS`; two prior design docs contradict it

ADR 0005 ("redb Index with QueryOps Namespace...", accepted 2026-07-27) is
the architecture `spec.md`'s "Further Notes" explicitly claims to follow. Its
decision section specifies **two** tables:

> "`file_records` (path → {...}) for every file, and `note_metadata`
> (path → {frontmatter, inline_fields, tags, **tasks, lists**, links}) for
> markdown files only."

`tasks` and `lists` were already planned as fields *inside* the note-metadata
record, not a third table. The earlier design document,
`.scratch/task-system/grilling-session.md` (19-round grilling session that
predates the spec), independently confirms the same intent under "Indexing":

> "Tasks stored in redb `note_metadata` table as part of Note serialization."

Issue 07 added a third table (`LISTS`) without an ADR amendment, without
updating `index/CONTEXT.md`, and — per Findings 1–2 — without a working
consumer or a correct serialization shape. `spec.md`'s claim that "this spec
follows the accepted redb/FileIndex and QueryOps architecture from the
existing ADRs" is not accurate as implemented. This should either become its
own ADR (documenting *why* a flat table is worth the write-amplification and
serialization redesign over the originally-accepted embedded-in-`NOTES`
approach — the "answer queries without deserializing full Notes" rationale
is plausible but was never written down or validated), or the table should
be removed and the original ADR 0005 schema followed as accepted.

## Finding 6 — Ticket 08 names a type that doesn't exist

`src/query/CONTEXT.md` (the authoritative, current domain glossary for the
query module) is explicit:

> **Query Row** — "A single query result row pairing a Note with its File
> Base, task state, and resolved field paths." *Avoid: Query Record, record,
> IndexRecord, QueryOutcome, page*

The actual code type is `QueryRow` (`src/query/results.rs`). ADR 0005 uses
the older, now-explicitly-deprecated name `IndexRecord` (point 8: "A single
record is `IndexRecord`") — itself stale and never updated when the type was
renamed. Ticket 08 uses a **third**, different name that matches neither:

> "Enrich `QueryRecord` with full task fields..." (issue 08, line 5)

`QueryRecord` does not exist anywhere in the codebase (`grep -rn
"QueryRecord" src` returns nothing) and isn't even the old ADR name — it's a
distinct, apparently invented label. An agent implementing ticket 08 exactly
as written would either fail to find the type or have to guess it means
`QueryRow`. This is a mechanical drafting bug, but it's a hard blocker: fix
the ticket text before assigning it.

## Finding 7 — Domain glossary was never updated as the spec required

`spec.md`'s own "Further Notes" section states:

> "The task system should update the project domain glossary with explicit
> Task, List Item, Checkbox, Task Status, Task Priority, Task Dates, and
> Fully Complete terms."

After issues 01–07 (all merged to `main` except 07, which is complete on its
branch), `src/note/CONTEXT.md`'s only relevant entries are still:

```text
#### Task
A checklist item carrying a configurable Task Status symbol (`[ ]`, `[x]`,
`[-]`, ...), description text, and optional date-shorthand emoji markers.
```

This is the **pre-issue-05/06** model. It doesn't mention `List Item` as the
primary structural type (issue 05's own spec text: "Keep List Item as the
primary structural type. A Task is not a separate tree; task-specific data
is composed into a List Item"), doesn't mention `Checkbox`, `Task List Item`,
`Task Priority`, `Task Dates`, or `Fully Complete` at all — every one of the
terms the spec explicitly named. This isn't a hypothetical gap; it's the
project's own acceptance criterion, unmet across seven issues' worth of
implementation. Whoever picks up issue 08 should not add `task.priority`,
`task.due`, etc. to the query layer while the note-domain glossary that's
supposed to define what those words *mean* still describes the old model.

## Finding 8 — `ListRecord.path` duplication is speculative, given Finding 1

Each `LISTS` row stores its own copy of the note's path as a `String`
(`ListRecord { path: String, item: ListItem }`), in addition to encoding it
as the key prefix (`list_key = path bytes + big-endian line`). For a note
with 50 list items, the same path string is serialized 51 times (once per
key, once per value) purely so that a *global* scan
(`read_all_lists`/`read_lists`, iterating across every note) can recover
which note a row belongs to without decoding the key.

This is not wrong in isolation — a flat global scan legitimately needs the
path somewhere — but per Finding 1, nothing performs a flat global scan
today. `QueryRow`'s actual file/note-field join mechanism
(`position: RowIndex` into the live, in-memory `FileIndex.entries`) is O(1),
zero-copy, and needs no path string at all. The `path`-carrying design only
pays for itself if `LISTS` becomes a genuine standalone/lazy read path
(loading list-level data *without* deserializing full `Note`s) — a decision
that was never made or written down (Finding 5). Until that's a real,
justified design (with a benchmark showing it beats the current
fully-in-memory `FileIndex` approach — `benches/index_lifecycle.rs` already
has the harness for this), the per-row `path` field is complexity paid for a
capability nothing uses.

**The join mechanism itself is worth keeping as-is.** `QueryRow`'s
position-based join (not "one giant struct with everything," not "path key
against the FILES/NOTES table") is a legitimately deep design — it's the
answer to the user's "best way to include the file/note level data fields"
question, and it's already correct. Preserve it for any `ListRow` redesign:
`ListRow`, like `TaskRow` today, should keep carrying only the list-item
delta and resolve `file.*`/frontmatter fields through the same
`position: RowIndex` the row already holds — it should **not** grow its own
path-keyed lookup into `FILES`/`NOTES` in parallel.

## Finding 9 — `"list"` is already a four-way naming collision

1. **CLI**: `traces list` (`src/cli/list.rs`) — lists *Notes* (page rows).
2. **Template filter**: `list` / `list_filter` (`src/template/engine/query.rs`)
   — renders any `QuerySet` as a markdown bullet list, regardless of row
   kind.
3. **Domain model**: `List` / `ListItem` / "List Item" — the markdown bullet
   structure itself (`src/note/lists.rs`).
4. **Proposed new surface** (Finding 4): `ListRow` / `QueryMode::Lists` /
   `Lists` table — query-level "all list items regardless of task-ness."

Four unrelated meanings for the same word, in the same crate, three of which
already ship. Introducing a fifth without deliberately disambiguating (the
`index/CONTEXT.md` and `query/CONTEXT.md` "*Avoid*:" convention already
exists for exactly this purpose — use it) will make onboarding and code
review measurably harder. Recommend the domain glossary entries for any new
`ListRow`/`Lists`-mode terms spell out the collision explicitly, e.g. "*Avoid
bare 'list' in this context; always say 'List Item row' — 'list' alone means
the `traces list` page command or the `list` render filter elsewhere in this
codebase.*"

## Finding 10 — Visibility: `ListRecord` is `pub`, its producer isn't

`ListRecord` is unconditionally exported: `src/lib.rs`: `pub use note::{...,
ListRecord, ...}`. Its only real producer, `IndexerService::read_lists()`,
sits behind `IndexerService`, which is `pub(crate)` in non-test builds
(`#[cfg(not(any(test, feature = "test-utils")))] pub(crate) use
index::IndexerService;`). An external consumer of this crate as a library
can see and construct a bare `ListRecord` but has no way to obtain a real one
from the index in a release build. Low severity (nothing is broken today,
since nothing external is trying), but worth folding into the Finding 3
rename/relocation — a `ListEntry` that only ever exists as a return value of
a crate-internal method probably shouldn't be unconditionally `pub` either;
match `FileEntry`'s existing visibility pattern (`pub` only under
`test`/`test-utils`, per `src/lib.rs:104-106`).

## Finding 11 — "Backward compatibility" language in ticket 08 contradicts the spec

Ticket 08: *"Old `task.completed` and `task.text` fields remain available
for backward compatibility."* Spec `spec.md`, "Out of Scope": *"Backward
compatibility with shipped external consumers, because the project has not
shipped yet."* There's nothing external to be backward-compatible *with* —
what's actually meant is "keep the existing field names working alongside
the new ones for template-authoring continuity within this pre-release
codebase." Reword to avoid implying a compatibility guarantee the project
has explicitly disclaimed.

---

## Suggested resequencing

1. **Amend issue 07 before merging** (it's still on an unmerged branch,
   zero external consumers, small blast radius):
   - Rename `ListRecord` → `ListEntry`; move to `src/index/entry.rs`
     (Finding 3); align its visibility with `FileEntry` (Finding 10).
   - Fix the `LISTS` row serialization to not embed descendant subtrees
     (Finding 2) — or, if the table is kept, at minimum land this as a
     documented, deliberate tradeoff with a benchmark, not silently.
   - Decide, explicitly and in writing (ADR or spec amendment), whether
     `LISTS` is (a) removed in favor of the ADR-0005-accepted
     embedded-in-`NOTES` design (Finding 5), since nothing currently reads
     it (Finding 1), or (b) kept and *actually wired into* `query_tasks`/a
     new list-level query path as the reason for its existence. Either
     answer is defensible; leaving it unresolved (current state) is not.
   - Update `index/CONTEXT.md` and `note/CONTEXT.md` per the spec's own
     mandate (Finding 7).
2. **Rewrite issue 08** around `RowKind::List(ListRow)` +
   `QueryMode::Lists`/`Tasks` (Finding 4) instead of extending `TaskRow` in
   place, and fix the `QueryRecord` → `QueryRow` naming (Finding 6) and the
   "backward compatibility" wording (Finding 11).
3. Tickets 09–11 need no structural changes — they consume `task.<field>`
   field paths, which stay stable under the `ListRow` redesign (Finding 4).
