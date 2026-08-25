# 14 — Deepen IndexStore's Redb Adapter, Reshape RefreshCache's Storage Access, and Persist on Refresh

**What to build:** Collapse `src/index/store.rs`'s six near-identical postcard
encode/decode methods (`store_table`, `load_table`, `store_links`,
`load_links`, `upsert_row`, `load_note`) behind one small `Postcard<T>`
type implementing `redb::Value`; retype `FILES`/`NOTES`/`LINKS` to `&[u8]`
keys (and, for `LINKS`, `&[u8]` values) instead of `path.to_string_lossy()`;
transparently rebuild `.traces/index.redb` on a detected schema mismatch
or structural corruption; split `Durability` between the frequent
incremental write path and the explicit full-rebuild path. Make
`IndexerService::refresh()` persist its own result internally (best-effort,
never failing the caller's `refresh()` on a persist error) instead of
requiring a separate `persist()` call every caller today forgets to make —
and do so through exactly **one** `IndexStore::open()` and **one** read
transaction per `refresh()` call, not the two-and-two a naive
implementation would reach for. Also resolves, mitigates, or explicitly
and reasonedly accepts every `clippy::large_stack_frames` warning
`src/index/store.rs` and `src/index/service.rs` raise, re-verified against
the code shape this ticket actually produces (see Design Decision 12 —
the specific byte counts below are stale the moment this ticket's other
changes land, and must be re-measured, not assumed).

**Related to:** architecture review of `src/index/` (this repo's own
codebase-design skill run), across two grilling passes. The first pass
settled the redb-adapter/persist-on-refresh work below (Design Decisions
1–12). A second, later pass — grilling `service.rs`/`builder.rs`/`scan.rs`
together, informed by research against `salsa`, `tantivy`, git's index,
and Obsidian/Logseq's link-graph designs — found that the first pass's
"make `refresh()` persist internally" plan, implemented as originally
written, would have silently reintroduced a measured ~20% latency
regression (a second, redundant `IndexStore::open()`) and a second,
redundant read transaction. Design Decisions 13–17 below are that second
pass's findings, folded into this ticket rather than filed separately,
because they change the exact shape of `RefreshCache` and `refresh()`
this ticket already owns. Ticket 15 is the deliberately-deferred,
separately-scoped follow-up that reshapes `IndexBuilder` itself and gives
`RefreshCache` its own methods — land this ticket first; ticket 15 builds
directly on the `RefreshCache` shape (borrowed fields, boxed, built via
`RefreshCache::load`) this ticket introduces. Ticket 16 (`LINKS`
reconstruction fidelity for non-Unicode filenames) is unaffected in
scope but needs one reference updated — see its own file.

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
      `builder.rs`/`entry.rs`/`delta.rs` (ticket 15) depends on `Path::cmp`
      order specifically; deleting the sort would silently break them.
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
      `RefreshCache.previous` naturally makes every current file
      "upserted", and — because the tables genuinely are empty after a
      wipe, not merely assumed empty — `deleted` is correctly `[]` too;
      see Design Decision 17 for why that distinction matters).
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
      entirely when the computed `IncrementalDelta` is empty. This check
      is `IncrementalDelta::is_empty(&self) -> bool` — a method on the
      type itself (`self.upserted.is_empty() && self.deleted.is_empty()
      && self.links_deleted.is_empty() &&
      self.links_upserted.as_ref().is_none_or(Vec::is_empty)`), not
      inline boolean logic in `store.rs` reaching into four fields it
      doesn't own. `IncrementalDelta` lives in ticket 15's new `delta.rs`
      by the time this lands there — if ticket 15 hasn't landed yet,
      define `is_empty` on it in `builder.rs` where it currently lives
      and let ticket 15 move it, not re-derive it.
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
- [ ] `IndexStore` gains `load_bases_and_links_via(&self, read_txn:
      &ReadTransaction) -> Result<(Vec<FileBase>, InlinkMap), IndexError>`
      — identical body to today's `load_bases_and_links`, except it takes
      the transaction instead of opening one via `self.begin_read()`. The
      old self-opening `load_bases_and_links()` is **removed**, not kept
      alongside it — confirmed via `references` search that its only
      callers (`IndexerService::refresh` and a `reuse_state` test helper
      in `builder.rs` mirroring production usage) both immediately called
      `store.begin_read()` again right after, so nothing depends on the
      self-opening version's existing behavior.
- [ ] `RefreshCache` (`src/index/builder.rs`) gains a lifetime parameter
      and borrows instead of owns: `store: &'a IndexStore`, `read_txn:
      &'a ReadTransaction` replace the previously-owned `IndexStore`/
      `ReadTransaction` fields. `RefreshCache::load(store: &'a IndexStore,
      read_txn: &'a ReadTransaction) -> Result<Self, IndexError>` becomes
      its only constructor (all fields stay private) — it calls
      `store.load_bases_and_links_via(read_txn)` to populate `previous`/
      `inlinks`, then stores the two borrows alongside them. `IndexBuilder`
      threads the same `'a` through (`IndexBuilder<'a>`); its `cache`
      field becomes `Option<Box<RefreshCache<'a>>>` — boxed for the same
      reason `IndexDelta` already boxes its `Incremental` payload (see
      Design Decision 13), just applied to a now-borrowed-and-much-smaller
      struct instead of the previously-owned one. Renaming
      `reuse_unchanged` to `with_cache` and giving `RefreshCache` its own
      methods (`diff_bases`, `reconcile_note`, `diff_links`,
      `into_inlinks`) instead of `build_with_reuse` reaching into its raw
      fields is ticket 15's job, landing on top of this — this ticket only
      needs `RefreshCache`'s fields borrowed, boxed, and constructed via
      `load()`; leave `reuse_unchanged`'s name and `build_with_reuse`'s
      body (reaching into `cache.previous`/`&cache.store`/`&cache.read_txn`
      directly, as today) otherwise alone. `reuse_unchanged`'s own
      parameter (currently four loose parameters, becoming one
      `RefreshCache` per this ticket) is also renamed from `reuse` to
      `cache` for the same reason: it's the same value the field stores,
      and having the field called `cache` while every local reference to
      its unwrapped value stayed called `reuse` would be confusing, not
      preserved-for-a-reason.
- [ ] `IndexerService::refresh()` is rewritten to open the store once,
      scope the read transaction to a block that ends before persisting:
      ```rust
      pub fn refresh(&self) -> Result<FileIndex, IndexError> {
          let store = IndexStore::open(&self.root)?;
          let index = {
              let read_txn = store.begin_read()?;
              let cache = builder::RefreshCache::load(&store, &read_txn)?;
              builder::IndexBuilder::from_scan(&self.root)?
                  .reuse_unchanged(cache)
                  .build(&self.root)?
          }; // read_txn (and cache, which borrows it) drop here
          if let Err(source) = store.persist_index(&index) {
              tracing::warn!(%source, "failed to persist refreshed index");
          }
          Ok(index)
      }
      ```
      (`reuse_unchanged` here takes the whole `RefreshCache` as one
      argument, not four loose parameters — a narrower, incidental
      signature change riding along with the borrowing change; ticket 15
      renames the method itself to `with_cache`.) The read transaction
      opened for reconciliation never overlaps the write transaction
      `persist_index` opens — confirmed by construction, not by
      inspection, since `read_txn`'s block-scope drop is a hard Rust
      guarantee, not a convention to remember.
- [ ] `IndexDelta` (`src/index/builder.rs`, moving to `delta.rs` under
      ticket 15) gains a doc comment recording why `Full` and
      `Incremental` are not interchangeable, even when an `Incremental`
      diff would come out empty — see Design Decision 17 for the exact
      wording and the hazard it documents.
- [ ] A new test: persist an index, delete a note, call `refresh()`
      (which now persists internally per this ticket), then `load()`
      fresh and assert the deleted note's `FileBase`/`Note`/inlink rows
      are actually gone from disk — not merely absent from `refresh()`'s
      in-memory return value. `persists_rebuilds_rather_than_appends`
      already proves this for the `Full`/`replace_all` path
      (`build()`+`persist()`); nothing currently proves it for the
      `Incremental`/`persist_incremental` path, which becomes the common
      case the instant `refresh()` persists on every call. See Design
      Decision 17.
- [ ] `clippy::large_stack_frames` on `load_note` (currently 4212 bytes)
      is resolved once its raw `postcard::from_bytes` call and
      `DbError::Deserialize{path, source}` construction move behind
      `Postcard<T>::from_bytes` instead of living inline in `load_note`'s
      own body — confirmed empirically: extracting exactly that logic
      into a standalone `deserialize_row` helper (a plain function, not
      yet the real trait impl) made the warning disappear entirely in an
      isolated, reverted experiment. Verify against the actual
      `Postcard<T>` implementation once built, since the trait-dispatch
      mechanism isn't byte-identical to the experiment's plain helper.
- [ ] `clippy::large_stack_frames` on `load_table<T>` (currently 4495
      bytes) is **not assumed fixed** by `Postcard<T>` — the same
      extraction pattern that fixed `load_note` was also tried on
      `load_table` in isolation and made it *worse* (4495 → 4575 bytes),
      because `load_table` deserializes inside a loop, not at a single
      point lookup: a naive per-row helper call adds a new intermediate
      `PathBuf` and a call boundary rather than removing overhead. After
      implementing the real `Postcard<T>` (whose mechanism differs from
      the naive experiment — dispatch happens via redb's own iterator,
      not an explicit per-row function call), re-run clippy and treat
      the result as new information, not a foregone conclusion either
      way.
- [ ] `load_links` (currently the module's worst offender at 7957 bytes)
      is decomposed into three named steps instead of one function doing
      table-iteration-error-handling, target extraction, and nested
      source-collection all inline: (1) the existing per-target loop,
      (2) a `process_link_entry`-shaped helper handling one
      `(AccessGuard, MultimapValue)` row (unwrap, extract target,
      delegate to the source collector), (3) a `collect_sources`-shaped
      helper draining one target's source multimap-value into a
      `Vec<PathBuf>`. Confirmed empirically: this two-level extraction
      cuts the reported frame from 7957 to 4405 bytes (44%) in an
      isolated, reverted experiment — a real, validated reduction, not
      full compliance (still ~309 bytes over the 4096 threshold). Do
      **not** chase further micro-extraction purely to cross the
      threshold number; if 4405-ish bytes remains after this ticket's
      other changes (byte keys, `Postcard<T>`, `load_bases_and_links_via`)
      are also applied, accept the residual with a documented,
      narrowly-scoped `#[expect(clippy::large_stack_frames, reason =
      "...")]` on `load_links` itself, rather than fragmenting the
      function further for its own sake. This decomposition is also the
      natural seam ticket 16's later correlation-against-loaded-notes
      rewrite can build on, rather than restructuring an even-more-tangled
      starting point.
- [ ] `replace_all` and `persist_incremental` each get a documented,
      narrowly-scoped
      `#[expect(clippy::large_stack_frames, reason = "...")]` directly
      on the function itself, instead of further decomposition —
      investigated and empirically tested, not assumed: both are
      dominated by `redb::WriteTransaction` itself (624 bytes, confirmed
      via `size_of`), an external RAII type that must stay alive for the
      whole "open transaction, write N tables, commit" sequence by
      construction. Boxing does **not** help here — tested directly:
      `Box::new(self.begin_write()?)` made `replace_all` *worse* (4983
      → 5137 bytes), because `WriteTransaction` is constructed once and
      used within this one function, never moved through multiple
      owning calls the way the old, owned `RefreshCache` was — boxing it
      only adds a heap allocation on top of the same initial stack
      materialization, with no repeated-copy cost to eliminate in
      exchange. At ~5KB against a 1–8MB thread stack, this carries no
      real overflow risk; the `reason` string should say exactly this,
      and that boxing was tried and measured worse, so a future reader
      doesn't re-litigate either.
- [ ] `IndexerService::refresh`'s own `clippy::large_stack_frames`
      warning (previously resolved by boxing the *owned* `RefreshCache`,
      collapsing `IndexBuilder` from 304 to 32 bytes) is **re-measured
      after this ticket's borrowing change**, not assumed still fixed by
      the same reasoning. The borrowed `RefreshCache` is already much
      smaller before boxing (roughly 88 bytes: a 24-byte `Vec<FileBase>`,
      a 48-byte `InlinkMap`, and two 8-byte references, versus the
      previous ~200+ owned bytes) — boxing it is still free margin worth
      keeping, but confirm with `cargo clippy` against the actual
      implemented code that `refresh()`'s frame stays under threshold
      once both the borrowing change and the phase-split block-scoping
      above are in place, rather than carrying forward a byte count
      measured against a materially different struct shape.

## Comments

> *Filed after a full grilling session on this architecture-review
> candidate — decisions below are the user's confirmed answers, not
> proposals awaiting review. Decisions 1–12 are from the first grilling
> pass (redb adapter, persist-on-refresh); 13–17 are from a second pass
> that reconsidered `RefreshCache`'s ownership and construction before
> this ticket's plan was implemented.*

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
   during audit (Design Decision 11) to also catch structural corruption,
   not just schema mismatch.
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
12. **`clippy::large_stack_frames` triaged per-function, not
    blanket-suppressed, and not stopped at the first plausible-looking
    fix, and re-verified after every subsequent design change in this
    ticket** — ran clippy with the lint enabled (already `warn` in this
    project's `Cargo.toml`) across `src/index/`, found seven warnings
    across three files, and empirically tested every candidate fix
    rather than reasoning from first impressions: implemented each
    refactor in isolation, measured with `cargo clippy`, then reverted.
    The byte counts recorded in this ticket's checklist were measured
    against the code shape *before* the `RefreshCache`
    borrowing/boxing/phase-split changes (Decisions 13–15) existed —
    they are a methodology and a set of already-tested individual
    fixes, not a promise that the final numbers match once every change
    in this ticket is applied together. Re-run `cargo clippy` against
    the actual implemented code as the last step before closing this
    ticket, not as an assumption baked into the checklist.
13. **`RefreshCache` borrows `&IndexStore`/`&ReadTransaction` instead of
    owning them — a second grilling pass, after this ticket's plan
    (Decisions 1–12) was written but before it was implemented.**
    Measured directly (temporary in-process timing tests, reverted, not
    landed): `IndexStore::open()` costs ~3ms per call; the naive
    implementation of "refresh persists internally" — two independent
    `IndexStore::open()` calls, one for reconciliation, one for
    persisting — costs ~31–40ms for a 200-note steady-state refresh,
    over double a clean single-open ~14–17ms cost. `RefreshCache` owning
    the store/transaction by value is *why*: it gets moved into
    `IndexBuilder`'s self-consuming chain and is gone by the time
    `refresh()` would go on to persist, forcing a second `open()`.
    Borrowing keeps `refresh()`'s own `store` binding alive for the
    whole call, reused for both reconciliation and the final persist —
    eliminating the second `open()` entirely, not just making it cheaper.
14. **`load_bases_and_links` becomes `load_bases_and_links_via`, taking
    an external read transaction** — surfaced while designing
    `RefreshCache::load` as a constructor: today's `refresh()` opens
    *two* separate read transactions (one internal to the old
    `load_bases_and_links`, a second explicit one for point lookups
    during reconciliation), confirmed via `references` search to be true
    of every actual caller, not just a hypothetical. One shared
    transaction, held for the whole reconciliation phase, is strictly
    safer under redb's MVCC (both loads now observe the exact same
    snapshot) as well as cheaper.
15. **The read transaction's lifetime is explicitly scoped to end before
    `persist_index` opens a write transaction** — `RefreshCache`'s own
    prior doc comment already named the underlying concern (the
    transaction "pins the transaction's MVCC snapshot for the duration,
    deferring reclamation of pages superseded by writes made *during*
    this refresh," dismissed as "immaterial for this crate's single-shot
    CLI usage"). That dismissal was reasonable when refresh and persist
    were two separate, uncoordinated calls; it stops being reasonable
    the moment this ticket makes them happen inside one function.
    Cross-checked against `salsa`'s own architecture (rust-analyzer's
    incremental engine): salsa forcibly cancels and drops every
    outstanding read snapshot before allowing a mutation, treating
    read/write overlap against the same store as a structural hazard,
    not a per-callsite judgment call. traces-pkm doesn't have salsa's
    concurrent-reader problem (single-threaded CLI), but the block-scope
    fix costs nothing and removes the hedge from `RefreshCache`'s own
    doc comment instead of continuing to defer it.
16. **`IndexStore` is *not* embedded as a field of `IndexerService`** —
    considered directly: it would be the deepest possible fix to
    "how many times does this process open the database," since a
    long-lived `IndexerService` could open the store exactly once for
    its entire lifetime rather than once per `refresh()` call. Checked
    against actual usage before deciding, not assumed: `references`
    search across all 90 `IndexerService::new` call sites in `src/`,
    `benches/`, and `tests/` found zero cases where one `IndexerService`
    instance is reused across more than one store-touching method call
    — every caller constructs, calls one method, and drops. The cost is
    real and immediate: `redb::Database` does not implement `Clone`
    (confirmed against its generated docs), so embedding would force
    `IndexerService::new` to become fallible (`Result<Self, IndexError>`)
    — a mechanical but 90-call-site ripple — and would require either
    dropping `IndexerService`'s currently-`#[derive(Clone)]` (unused —
    zero actual `.clone()` call sites found) or wrapping `IndexStore` in
    `Arc` to keep it. The one scenario where embedding would pay off is
    a long-lived process reusing one `IndexerService` across many
    refreshes — which ADR-0005 explicitly names as deferred future work
    (`traces watch`, via the `notify` crate), not something this
    codebase does today. Deferred as YAGNI against a documented-but-
    unbuilt future, not implemented speculatively; revisit if/when
    `traces watch` is actually built.
17. **`IndexDelta::Full`/`Incremental` are documented as non-interchangeable,
    and a persisted-deletion round-trip test is added for the
    `Incremental` path specifically** — surfaced while confirming that
    `build_fresh` (which never touches `IndexStore` at all) can't safely
    be re-expressed as "incremental reconciliation against an empty
    `RefreshCache`," even though the two produce an identical in-memory
    `FileIndex` for that case. The persistence side does not: `Full`
    (`replace_all`) unconditionally wipes all three tables before
    rewriting, so it never needs to know what was deleted; `Incremental`
    (`persist_incremental`) only deletes paths its diff explicitly names,
    which is only correct because that diff is always computed against a
    `RefreshCache` loaded from the real, currently-persisted store (via
    `RefreshCache::load`, the only constructor — private fields make this
    a type-level guarantee, not just a convention) — never a synthetic or
    assumed-empty one. A `build_fresh` result tagged `Incremental` against
    a fabricated empty previous state would silently orphan any row for a
    file deleted since the last persist, because an empty diff can never
    produce a deletion. Concretely reachable today if this distinction
    were ever blurred: delete a note, re-run `traces index` — `build()`
    never opens the store, so it has no way to know that note's row
    exists. `persists_rebuilds_rather_than_appends` already proves this
    is safe for the `Full` path today; nothing currently proves the
    equivalent for `Incremental` once it becomes the common case (every
    `refresh()` call, per this ticket) — hence the new test above.

## Agent Brief

**Category:** enhancement

**Summary:** Deepen `IndexStore`'s redb adapter (value codec, byte-exact
path keys, transparent schema-mismatch/corruption recovery, tuned write
durability), make `IndexerService::refresh()` persist its own result
through exactly one store connection and one read transaction, and
resolve `src/index/`'s `clippy::large_stack_frames` warnings — measured,
not assumed, at every step, including after this ticket's own
`RefreshCache` reshaping.

**Current behavior:** `IndexStore` hand-writes postcard encode/decode at
six call sites keyed by `path.to_string_lossy()` (a lossy, collision-prone
`&str`). Schema drift or corruption in `.traces/index.redb` propagates as
a hard error with no recovery. `persist_incremental` and `replace_all`
share `Durability::Immediate`. Callers must remember to call `persist()`
after `refresh()` — most don't, so the on-disk cache only advances via
explicit `traces index` runs. `RefreshCache` owns an `IndexStore` and a
`redb::ReadTransaction` by value, moved through `IndexBuilder`'s
self-consuming chain; if `refresh()` were naively changed to persist
internally on top of today's shape, it would reopen the database a
second time and hold two separate read transactions, reproducing exactly
the ~20% latency regression this ticket's design avoids by construction.

**Desired behavior:** see the checklist above — this section intentionally
doesn't repeat it.

**Key interfaces:**

- `IndexStore` (`src/index/store.rs`) — `Postcard<T>: redb::Value`, byte
  keys, `open`'s eager mismatch/corruption probe, `Durability` split,
  new `load_bases_and_links_via(&self, read_txn: &ReadTransaction)`,
  removed `load_bases_and_links(&self)`.
- `RefreshCache` (`src/index/builder.rs`) — gains a lifetime parameter,
  borrows `&'a IndexStore`/`&'a ReadTransaction`, constructed only via
  `RefreshCache::load`. Its other methods (`diff_bases`, `reconcile_note`,
  `diff_links`, `into_inlinks`) and its move into a method-owning shape
  are ticket 15's scope, landing after this ticket.
- `IndexBuilder` (`src/index/builder.rs`) — `cache: Option<Box<RefreshCache<'a>>>`
  (was `Option<RefreshCache>`, owned, unboxed).
- `IndexerService::refresh`/`persist` (`src/index/service.rs`) — `refresh`
  persists internally, best-effort; `persist` unchanged.
- `IndexDelta` (`src/index/builder.rs`, moves to `delta.rs` under ticket
  15) — gains the non-interchangeability doc comment.

**Out of scope:**

- Renaming `reuse_unchanged` to `with_cache`, giving `RefreshCache` its
  own `diff_bases`/`reconcile_note`/`diff_links`/`into_inlinks` methods,
  the `IndexBuilder::new`/`with_cache`/`build` builder shape, backdated
  staleness (comparing outlink targets, not full note equality), moving
  `scan_root` into `IndexerService::scan`, and extracting `delta.rs` —
  all ticket 15, which lands on top of this ticket's `RefreshCache`
  shape.
- Embedding `IndexStore` in `IndexerService` — considered and rejected,
  see Design Decision 16.
- Ticket 16's `LINKS` byte-exact reconstruction — assumed to land after
  this ticket, per its own file.
