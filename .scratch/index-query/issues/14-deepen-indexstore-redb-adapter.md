# 14 — Deepen IndexStore's Redb Adapter and Persist on Refresh

**What to build:** Collapse `src/index/store.rs`'s six near-identical postcard
encode/decode methods (`store_table`, `load_table`, `store_links`,
`load_links`, `upsert_row`, `load_note`) behind one small `Postcard<T>`
type implementing `redb::Value`; retype `FILES`/`NOTES`/`LINKS` to `&[u8]`
keys (and, for `LINKS`, `&[u8]` values) instead of `path.to_string_lossy()`;
transparently rebuild `.traces/index.redb` on a detected schema mismatch
or structural corruption; split `Durability` between the frequent
incremental write path and the explicit full-rebuild path. Additionally,
make `IndexerService::refresh()` persist its own result internally
(best-effort, never failing the caller's `refresh()` on a persist error)
instead of requiring a separate `persist()` call every caller today
forgets to make.

**Related to:** architecture review of `src/index/` (this repo's own
codebase-design skill run). Fully grilled — every open branch below was
walked and settled interactively across two candidates, merged into one
ticket since they touch the same write path and one motivated the other
(the `Durability::None`/`Immediate` split only matters once
`persist_incremental` gets frequent, which the refresh-persists change
causes). Ticket 16 is the deliberately-deferred follow-up (`LINKS`
reconstruction fidelity for non-Unicode filenames) — do not fold it into
this ticket, see its own scope notes for why.

**Category:** enhancement

**Status:** ready-for-agent

- [ ] `Postcard<T>` (private, inline in `store.rs`) implements `redb::Value`
      (`fixed_width() -> None`, `from_bytes`/`as_bytes` delegating to
      `postcard::from_bytes`/`postcard::to_allocvec`, `type_name()` unique
      per `T`) and replaces the manual encode/decode in every table
      method that touches `FILES`/`NOTES` values.
- [ ] `FILES`, `NOTES`, `LINKS` all key on
      `path.as_os_str().as_encoded_bytes()` (`&[u8]`) instead of
      `path.to_string_lossy()` (`&str`); `LINKS`' values (source paths)
      move to `&[u8]` too.
- [ ] `LINKS` reconstruction on load (`load_links`) uses
      `str::from_utf8(bytes).map(PathBuf::from)`, falling back to
      `String::from_utf8_lossy` only when the bytes aren't valid UTF-8 —
      no `unsafe` anywhere in `src/index/`.
- [ ] `load_table`'s `items.sort_by(path_of)` pass **stays** — audit
      correction, not a deletion. `redb::ReadableTable::iter`'s
      "ascending order" guarantee describes redb's own byte-lexicographic
      key order, not `Path`/`PathBuf`'s component-wise `Ord`. These
      provably diverge: `Path::cmp("foo.md", "foo/bar.md")` is `Greater`;
      raw byte comparison of the same two strings is `Less` (confirmed by
      compiling and running the comparison) — an ordinary file next to a
      same-stemmed sibling directory, not a contrived input. Every
      downstream binary search, merge-join, and `debug_assert!` in
      `builder.rs`/`entry.rs` depends on `Path::cmp` order specifically;
      deleting the sort would silently break them.
- [ ] `IndexStore::open` eagerly probes all three tables via a read-only
      transaction immediately after opening the database; on
      `TableError::TableTypeMismatch`/`TypeDefinitionChanged` **or
      `DatabaseError::Storage(StorageError::Corrupted)`**, drops the
      handle, deletes the `.redb` file, and recreates it fresh under the
      new schema. No other call site (`store_table`, `load_table`, etc.)
      handles this — the probe in `open` is the only place it's checked.
      If the wipe-and-recreate itself fails (e.g. permission denied
      deleting the file), `open()` propagates a hard error — there is no
      fallback for a store that cannot be opened at all, unlike the
      best-effort persist-after-refresh case below.
- [ ] A test confirms the first `refresh()` after either a schema-mismatch
      or corruption recovery behaves like a full build (an empty
      `reuse.previous` naturally makes every current file "upserted").
- [ ] `persist_incremental`'s write transactions use
      `Durability::None`; `replace_all` (only reachable from the explicit
      `traces index` command) keeps the default `Durability::Immediate`.
- [ ] A test exercises the mismatch-and-rebuild path: open a `.redb` file
      written under the old `&str`/`&[u8]` schema, confirm `IndexStore::open`
      recovers by rebuilding rather than propagating `TableTypeMismatch`.
- [ ] `IndexerService::refresh()` persists its own result before
      returning — no new method, no per-call-site `persist()` call
      required. Its doc comment (currently "Returns the fresh FileIndex
      without persisting...") is rewritten to describe this.
- [ ] A persist failure inside `refresh()` is caught and logged via
      `tracing::warn!` (matching the existing schema-registry warning
      precedent in `src/cli/mod.rs`) and does not fail `refresh()`'s own
      `Result` — the caller still gets its fresh, correct in-memory
      `FileIndex`.
- [ ] `persist()`/`persist_incremental` skip opening a write transaction
      entirely when the computed `IncrementalDelta` is empty (`upserted`,
      `deleted`, and `links_deleted` all empty, `links_upserted` `None`
      or empty) — the common case once `refresh()` persists on every
      call.
- [ ] `benches/template_render.rs`'s `prepared_root()` builds and
      persists the index in its untimed setup step, matching the
      benchmark's own doc comment ("against a pre-built, pre-persisted
      1000-note project") for the first time — without this, the timed
      `render_to_file` call would measure a full rebuild-and-write on
      every iteration instead of a warm-cache render.
- [ ] No change needed at `src/cli/mod.rs`'s `refresh_page_query`/
      `refresh_task_query` or `src/template/engine/query.rs`'s
      `cached_refresh` — all three already call bare `refresh()`, which
      now persists internally; verify with a test that a `traces
      list`/`table`/`task` run advances `.traces/index.redb` without an
      explicit `traces index`.

## Comments

> *Filed after a full grilling session on this architecture-review
> candidate — decisions below are the user's confirmed answers, not
> proposals awaiting review.*

### Design Decisions (settled)

1. **Existing `.traces/index.redb` files on upgrade** — detect the type
   mismatch and transparently wipe + rebuild. It's a fully rebuildable
   derived cache; no versioned table names, no user-facing migration step.
2. **`LINKS` gets the same byte-key treatment as `FILES`/`NOTES`** —
   leaving it on `&str` would leave the exact collision risk this ticket
   exists to close still open in one of three tables.
3. **`Postcard<T>` lives inline in `store.rs`, private** — nothing else in
   the codebase touches redb; a new file would buy no reuse.
4. **`Durability::None` is split, not blanket** — `persist_incremental`
   (frequent, latency-sensitive now that `refresh()` persists on every
   call — settled below) gets it; `replace_all`
   (`traces index`, explicit, infrequent, user expects "it stuck" on
   return) stays `Immediate`.
5. **Mismatch/corruption detection is eager, centralized in
   `IndexStore::open`** — confirmed via `rust-docs-mcp` against redb 4.1
   source: type metadata (`TypeName`, width, alignment) is persisted per
   table and checked on `open_table`, returning a matchable
   `TableError::TableTypeMismatch`/`TypeDefinitionChanged`, never a
   panic; opening a table that doesn't exist always succeeds under
   whatever types are requested, so delete-and-recreate is a
   redb-idiomatic recovery. Reactive per-call-site handling was rejected
   as re-scattering the exact boilerplate this ticket removes. Broadened
   during audit (Design Decision 11 below) to also catch structural
   corruption, not just schema mismatch.
6. **No `unsafe`** — `FILES`/`NOTES` never need bytes→`Path`
   reconstruction at all (the authoritative `path` lives inside the
   postcard-decoded `FileBase`/`Note` value, not the key). `LINKS`
   does need it, but a safe `str::from_utf8` fast path (exact for every
   real-world path) plus a `from_utf8_lossy` fallback (identical to
   today's behavior, narrowed to only non-Unicode filenames) covers it
   without `OsStr::from_encoded_bytes_unchecked`. Full byte-exact
   fidelity for that narrowed edge case is ticket 16, deliberately not
   folded in here — see ticket 16's Triage Notes for the scope reasoning.
7. **Persist lives inside `refresh()` itself, not a new method or
   per-call-site opt-in** — initially considered gating it on the
   template engine's `WriteMode` (persist only for a real, non-dry-run
   render), reasoning a `--dry-run` preview shouldn't have side effects.
   Reconsidered: the only tested/documented dry-run guarantee
   (`dry_run_writes_nothing_even_when_output_already_exists`) is scoped
   to the *rendered output file*, not the index cache, and warming the
   cache during a dry-run preview has real value — a dry-run is often a
   rehearsal for a real render moments later. All callers persist
   uniformly.
8. **Failure handling is best-effort** — a persist failure (permission
   denied, disk full, race with another `traces index`) logs via
   `tracing::warn!` and does not fail the command. The query itself
   already succeeded against the fresh in-memory index; failing the
   whole command over a cache-warming side effect would be worse than
   today's behavior.
9. **Benchmark fixture fix is in scope, not a separate ticket** —
   `benches/template_render.rs` renders
   `{{ query.from() | list("file.path") }}` under `WriteMode::DryRun`,
   whose doc comment says this "isolates render cost from disk-write
   cost." Its `prepared_root()` setup never actually calls
   `build()`+`persist()` today despite claiming to (a pre-existing
   inaccuracy), so the timed render already pays a full 1000-note parse
   every iteration; once `refresh()` persists, it would also pay a full
   `replace_all` write unless the fixture's *untimed* setup does that
   persist first.
10. **`load_table`'s sort stays — audit correction, not a design choice**
    — originally scoped as a deletion on the strength of
    `ReadableTable::iter`'s "ascending order" guarantee. Re-derivation
    during a second grilling pass found that guarantee describes redb's
    own byte-lexicographic key order, not `Path`/`PathBuf`'s
    component-wise `Ord`, and the two provably diverge:
    `Path::cmp("foo.md", "foo/bar.md")` is `Greater`, raw byte comparison
    of the same two strings is `Less` (confirmed by compiling and running
    the comparison) — an ordinary file next to a same-stemmed sibling
    directory, not a contrived input. The sort is load-bearing, not dead
    code; every downstream binary search, merge-join, and
    `debug_assert!` in `builder.rs`/`entry.rs` depends on `Path::cmp`
    order specifically.
11. **Corruption recovery broadened beyond schema mismatch** — a
    follow-up fact-check on `Durability::None`'s crash safety confirmed
    redb is copy-on-write and crash-safe (a crash after a `None` commit
    cleanly reverts to the last durable state, no torn-write risk — the
    durability decision itself needed no change) but surfaced that
    genuine file corruption (`DatabaseError::Storage(StorageError::
    Corrupted)`) is a distinct error from `TableTypeMismatch`/
    `TypeDefinitionChanged`. Given the cache's whole recovery premise is
    "this file can never be worth failing a command over,"
    `IndexStore::open`'s wipe-and-rebuild now catches both, not just
    schema mismatch.

### Confirmed via `rust-docs-mcp` against redb 4.1 source (not guessed)

- `redb::Value`: custom types need `type SelfType<'a>`, `type AsBytes<'a>`,
  `fixed_width() -> Option<usize>`, `from_bytes`, `as_bytes`, `type_name()`.
- `redb::Key: Value` needs one method, `compare(&[u8], &[u8]) -> Ordering`;
  `&[u8]` implements it natively via byte-lexicographic order.
- `Durability::{None, Immediate}`, set via
  `WriteTransaction::set_durability`. `None`: "will not be persisted to
  disk unless followed by a commit with `Durability::Immediate`."
- `MultimapTable::remove_all` + loop-`insert` is already the idiomatic
  replace-one-key pattern; no bulk replace exists, not worth adding.
- `Database::compact()` requires exclusive `&mut Database` (blocks all
  readers) and redb's own source comments describe it as too slow to
  matter for this workload — not worth adopting.
- `ReadableTable::iter` docs: "Values are in ascending order" — this
  describes redb's own **key** order (byte-lexicographic), not
  `Path::cmp`'s component-wise order; the two diverge (see Design
  Decision 10). Originally, and wrongly, read as confirming
  `load_table`'s sort was dead code.
- redb is copy-on-write; a crash after a `Durability::None` commit but
  before a subsequent `Durability::Immediate` commit reverts cleanly to
  the last durable state — no torn-write risk.
- `DatabaseError::Storage(StorageError::Corrupted)` ("The Database is
  corrupted") is a distinct error variant from
  `TableError::TableTypeMismatch`/`TypeDefinitionChanged` — structural
  corruption is handled at the storage layer, schema mismatch at the
  table layer.
- `TableError::TableTypeMismatch`/`TypeDefinitionChanged` are ordinary
  `Result` variants; `open_table` creates a missing table under whatever
  types are requested.

## Agent Brief

**Category:** enhancement
**Summary:** Retype `IndexStore`'s three redb tables onto a single
`Postcard<T>` value codec and byte-exact path keys, add transparent
schema-mismatch and corruption recovery, tune write durability for a
workload that's a disposable, rebuildable cache rather than a source of
truth, and make `IndexerService::refresh()` persist its own result so
the on-disk cache actually stays warm across CLI and template queries
instead of only advancing on an explicit `traces index`.

**Current behavior:** `src/index/store.rs`'s `FILES`/`NOTES` tables are
`TableDefinition<&str, &[u8]>`; `LINKS` is
`MultimapTableDefinition<&str, &str>`. Every value is manually
postcard-encoded/decoded across six near-identical methods. Every key is
`path.to_string_lossy()` — lossy for non-UTF-8 paths, a latent collision
risk (two different non-UTF-8 paths can map to the same lossy string and
overwrite each other's row). `load_table` deserializes every row and
re-sorts it by `Path::cmp` order — necessary, not redundant, since
redb's own key order is byte-lexicographic on the encoded path bytes and
provably diverges from `Path::cmp`'s component-wise order (see Design
Decision 10).
`IndexStore::open` uses the default `Durability::Immediate` for
every write, including the frequent small writes `persist_incremental`
makes, even though `.traces/index.redb` is fully regenerable by
rescanning the project's markdown files.

Separately, `IndexerService::refresh()` computes a fresh `FileIndex` but
documents that it does *not* persist — callers must call `persist()`
themselves. No production caller does: `src/cli/mod.rs`'s
`refresh_page_query`/`refresh_task_query` (backing `traces
list`/`table`/`task`) and `src/template/engine/query.rs`'s
`cached_refresh` (backing the `query`/`tasks` template namespace) all
call bare `refresh()` and discard the result without persisting. Only the
explicit `traces index` command (`build()`+`persist()`) advances the
on-disk cache, so every query against an edited vault re-parses the same
changed notes on every invocation until the user manually reindexes.

**Desired behavior:** see the checklist above — this section intentionally
doesn't repeat it.

**Key interfaces:**

- `IndexStore` (`src/index/store.rs`) — all six table-mechanics methods,
  `open`, `replace_all`, `persist_incremental` and its write-transaction
  construction.
- `FILES`/`NOTES`/`LINKS` table `const`s — type signatures change; every
  call site that opens them follows.
- `IndexerService::refresh()` (`src/index/service.rs`) — gains an
  internal persist step and a doc-comment rewrite. `IndexerService::
  persist()`/`build()` are unchanged.
- `benches/template_render.rs` — `prepared_root()` gains a build+persist
  step in its untimed setup.
- No change to `IndexBuilder`, or to any table-mechanics type beyond
  `IndexStore`/`IndexerService` — this ticket is scoped to the
  persistence adapter and the one method that calls it, not the
  scan/parse/reconcile pipeline.

**Out of scope:**

- Ticket 16 (`LINKS` reconstruction fidelity via correlation against
  loaded `notes`, closing the `str::from_utf8` fallback's residual gap).
- Any change to what gets stored (`FileBase`, `Note`, `InlinkMap` shapes)
  — only how it's encoded and keyed changes.
