# 15 — Rebuild IndexBuilder as a Proper Cache-Driven Builder, with Backdated Staleness

**What to build:** Reshape `src/index/builder.rs` around three findings
from a second grilling pass (after ticket 14's plan was written but before
either ticket was implemented):

1. `IndexBuilder::build_with_reuse` walks `(bases, cache.previous)` twice,
   independently — once in `diff_bases`, again via `has_deleted_note`+
   `reconcile_note` sharing a fresh `prev_iter`. Fold into one pass.
2. `IndexBuilder` reaches directly into `RefreshCache`'s raw fields
   (`&cache.previous`, `&cache.store`, `&cache.read_txn`, `cache.inlinks`)
   at four different points across `build_with_reuse` — a shallow
   interface, not a cohesive one. Give `RefreshCache` its own methods
   (`diff_bases`, `reconcile_note`, `diff_links`, `into_inlinks`) so
   `IndexBuilder` only ever calls it, never reaches into it.
3. `dirty` (staleness) is set unconditionally whenever a Note is
   reparsed — conflating "this Note's *content* changed" with "this
   Note's *outlinks* changed," when only the latter should ever force a
   full `derive_inlinks` recompute. **Backdate**: for a reparsed Note,
   compare its new outlink targets against its previous persisted
   value (one extra point lookup, already-paid machinery) and only
   contribute to staleness if they actually differ.

Alongside this: `IndexBuilder` becomes a proper builder (`new`/
`with_cache`/`build`, not `from_scan`/`reuse_unchanged`/`build`) —
scanning moves out of it entirely, into `IndexerService::scan`
(`scan.rs` is deleted). `build_with_reuse` is renamed `build_with_cache`.
`IndexDelta`/`IncrementalDelta`/`diff_bases`/`diff_inlinks` move into a
new `delta.rs`, and `IncrementalDelta` gains an `is_empty()` method.

**Related to:** the same architecture-review grilling session as ticket
14 — this ticket lands *after* it. Ticket 14 already reshapes
`RefreshCache`'s fields (borrowed, boxed, constructed only via
`RefreshCache::load`) and renames `IndexBuilder`'s `reuse` field to
`cache`; this ticket gives `RefreshCache` its methods and rewrites
`IndexBuilder`/`IndexerService` around them. Independent of ticket 16
(different file, different motivation — `LINKS` reconstruction fidelity,
not reconciliation-algorithm shape).

**Category:** enhancement

**Status:** completed

## `delta.rs` extraction

- [x] New file `src/index/delta.rs`. `IndexDelta`, `IncrementalDelta`,
      `diff_bases`, `diff_inlinks` move here from `builder.rs` verbatim,
      except where the checklist below changes them. `mod.rs` gains
      `mod delta;`.
- [x] `IndexDelta`'s doc comment carries forward unchanged from ticket
      14's Design Decision 17 (the `Full`/`Incremental`
      non-interchangeability explanation) — do not lose it in the move.
- [x] `IncrementalDelta::is_empty(&self) -> bool` — `self.upserted.is_empty()
      && self.deleted.is_empty() && self.links_deleted.is_empty() &&
      self.links_upserted.as_ref().is_none_or(Vec::is_empty)`. Used by
      ticket 14's "skip the write transaction when the delta is empty"
      checklist item; define it here if ticket 14 hasn't landed yet
      rather than duplicating the logic in `store.rs`.
- [x] `diff_bases` signature changes to
      `pub(super) fn diff_bases(current: &[FileBase], previous: &[FileBase])
      -> (Vec<PathBuf>, Vec<PathBuf>, bool)` — `upserted`, `deleted`, and a
      third value named `has_deleted_note` (**not** a general "is anything
      stale" flag — see the backdating section below for why this is
      narrower than it might first look). `has_deleted_note` is `true`
      iff any *deleted* entry (present in `previous`, absent from
      `current`) was `FileFormat::Note`. This is computed within
      `diff_bases`'s existing merge pass at zero extra cost, folding in
      what `has_deleted_note` (the old free function) used to compute
      separately — a deleted Note always forces an inlink recompute, and
      this is knowable from `FileBase` metadata alone, no Note content
      needed.
- [x] The old free-standing `has_deleted_note` function is deleted — its
      job is fully absorbed into `diff_bases`.
- [x] `diff_inlinks` is unchanged (still diffs two `InlinkMap`s by
      source-set membership) — only its file location moves.
- [x] `diff_bases` and `diff_inlinks` gain direct unit tests in
      `delta.rs` (currently `diff_bases` is only exercised indirectly via
      `build_with_reuse`/`refresh()` integration tests): a deleted Note
      sets `has_deleted_note`; a deleted non-Note file does not; an
      upserted Note does **not** set `has_deleted_note` (that's the point
      — upserted-Note staleness is decided elsewhere now, see below).

## `RefreshCache` gains methods (`IndexBuilder` stops reaching into its fields)

- [x] `RefreshCache` (fields, the `cache` field name on `IndexBuilder`,
      and `load` constructor already land via ticket 14) gains:
      ```rust
      impl<'a> RefreshCache<'a> {
          /// Diffs `current` against the previous scan.
          fn diff_bases(&self, current: &[FileBase]) -> (Vec<PathBuf>, Vec<PathBuf>, bool) {
              delta::diff_bases(current, &self.previous)
          }

          /// Reuses `base`'s Note via point lookup when unchanged;
          /// otherwise reparses, then backdates (see below). Returns
          /// whether this Note's outlinks actually changed.
          fn reconcile_note(
              &self,
              base: &FileBase,
              is_upserted: bool,
              root: &Path,
          ) -> Result<(Note, bool), IndexBuilderError> { ... }

          /// Diffs the previous inlink map against a freshly recomputed one.
          fn diff_links(&self, current: &InlinkMap) -> (Vec<PathBuf>, Vec<PathBuf>) {
              delta::diff_inlinks(&self.inlinks, current)
          }

          /// Consumed when nothing was stale: hands back the previous
          /// inlink map unchanged.
          fn into_inlinks(self) -> InlinkMap {
              self.inlinks
          }
      }
      ```
      `diff_bases`/`diff_links` are thin wrappers delegating to `delta.rs`'s
      pure functions — `delta.rs` keeps the algorithms independently
      testable with no `IndexStore`/`ReadTransaction` fixture; `RefreshCache`'s
      methods are the ergonomic binding against its own held previous-state.
      Two real shapes for the same logic (direct unit tests in `delta.rs`,
      integration coverage through the full reconciliation flow), not one
      dressed up as a method for its own sake.
- [x] `IndexBuilder`'s own `reconcile_note` associated function is
      **deleted entirely** — it moves into `RefreshCache` as above, not
      duplicated. `IndexBuilder`'s impl block shrinks correspondingly;
      confirm no leftover unused-import or dead-code warnings after
      removal.
- [x] `parse_note` stays a free function in `builder.rs`, **not** a
      `RefreshCache` method — it's called by both `build_fresh` (no
      `RefreshCache` involved at all) and `RefreshCache::reconcile_note`.
      This is the one case where a genuine second caller justifies a
      shared free function rather than either type owning it.

## Backdating (the actual staleness fix)

- [x] `RefreshCache::reconcile_note`'s reparse branch (`is_upserted ==
      true`) additionally point-looks-up the *previous* Note at
      `base.path()` via `self.store.load_note(self.read_txn, base.path())`
      — the same mechanism the unchanged branch already uses — and
      compares outlink **targets only**, not full `Note` or `Link`
      equality. `Link` carries a display `text` field alongside `target`;
      comparing full `Link` equality would under-fire backdating on the
      common case of a user renaming a wikilink's display text
      (`[[note|new label]]`) without moving its target. Compare a
      deduplicated, sorted collection of `Link::target()` strings from
      each side (deduplication matches `derive_inlinks`'s own documented
      behavior: "duplicate outlinks to the same target within one Note
      ... collapse to a single edge" — two notes with the same target set
      in different textual order, or with an incidental duplicate, must
      compare equal).
- [x] Three outcomes for the reparse branch, each returning `(Note, bool)`
      where `bool` is "outlinks changed":
      - Previous Note exists at this path, target sets match →
        `(new_note, false)` — backdated; the Note's row is still
        upserted (its content did change), but it does not force an
        inlink recompute.
      - Previous Note exists, target sets differ → `(new_note, true)`.
      - No previous Note at this path (a genuinely new file) →
        `(new_note, true)` — nothing to backdate against, always counts.
- [x] **Backdating's point lookup fails open, never hard-errors the
      refresh.** If loading the previous Note for comparison errors for
      any reason other than "not found" (e.g. a corrupted or
      undeserializable stored row), treat it identically to "outlinks
      changed" — log via `tracing::debug!` and continue, do not propagate
      an error that would fail the whole `refresh()`. Backdating is a
      pure optimization (skipping unnecessary work); its failure must
      never turn an otherwise-successful refresh into a hard error. The
      Note's *own* fresh value was already successfully parsed via
      `parse_note` regardless of whether this comparison succeeds — only
      the "can we skip the inlink recompute" decision is at stake, never
      correctness of the returned `FileIndex`.
- [x] `IndexBuilder::build_with_cache` (renamed from `build_with_reuse`)
      assembles `stale` starting from `diff_bases`'s `has_deleted_note`,
      then `|=`s in each reconciled Note's `outlinks_changed`:
      ```rust
      fn build_with_cache(
          bases: Vec<FileBase>,
          root: &Path,
          cache: RefreshCache<'a>,
      ) -> Result<FileIndex, IndexBuilderError> {
          let (upserted, deleted, mut stale) = cache.diff_bases(&bases);
          let mut upserted_iter = upserted.iter().peekable();
          let mut notes = Vec::with_capacity(bases.len());

          for base in &bases {
              let is_upserted =
                  upserted_iter.next_if(|p| p.as_path() == base.path()).is_some();
              if base.format() != FileFormat::Note {
                  continue;
              }
              let (note, outlinks_changed) = cache.reconcile_note(base, is_upserted, root)?;
              stale |= outlinks_changed;
              notes.push(note);
          }

          let new_inlinks_if_stale = stale.then(|| derive_inlinks(&notes));
          let (links_upserted, links_deleted) = match &new_inlinks_if_stale {
              Some(new_map) => {
                  let (u, d) = cache.diff_links(new_map);
                  (Some(u), d)
              }
              None => (None, Vec::new()),
          };
          let inlinks = new_inlinks_if_stale.unwrap_or_else(|| cache.into_inlinks());

          let delta = IndexDelta::Incremental(Box::new(IncrementalDelta {
              upserted, deleted, links_upserted, links_deleted,
          }));
          Ok(FileIndex::new(bases, notes, inlinks, delta))
      }
      ```
      The upserted-membership check — and consuming a match — runs
      unconditionally for every `base`, Note or not, before the
      format-gated `continue`, exactly mirroring how the old
      `has_deleted_note` ran unconditionally: skipping it for non-Note
      bases would stall `upserted_iter`, misaligning it for later
      Note-format bases and reintroducing the exact double-counting bug
      class `builder.rs`'s own doc comments record having been bitten by
      once already.
- [x] Tests, each isolating one signal:
      - A Note's frontmatter/body/tasks change but its outlinks stay
        identical (same target set) → `refresh()`'s delta has
        `links_upserted: None` (inlinks reused, not recomputed) — proves
        backdating actually skips the recompute, not just that it's
        *possible* to skip it.
      - A Note's outlinks change (link added, removed, or retargeted) →
        `links_upserted`/`links_deleted` reflect it, same as before this
        ticket.
      - A Note's outlinks are reordered, or a wikilink's display text
        changes, with the same *target* set → still backdates (proves
        the comparison is target-set-based, not raw `Link`/`Vec` order
        equality).
      - A brand-new Note (no previous entry at its path) → always
        contributes to `stale`, never backdated.
      - Simulate a corrupted previous-Note row at a changed path (write
        invalid bytes directly into the `NOTES` table, bypassing normal
        persist) → `refresh()` still succeeds, with `stale` set (fail-open
        confirmed, not just assumed from the code).

## `IndexBuilder` becomes a proper builder; scanning moves out

- [x] `IndexBuilder` gains a lifetime parameter threading `RefreshCache`'s:
      ```rust
      pub(super) struct IndexBuilder<'a> {
          bases: Vec<FileBase>,
          cache: Option<Box<RefreshCache<'a>>>,
      }

      impl<'a> IndexBuilder<'a> {
          pub(super) fn new(bases: Vec<FileBase>) -> Self {
              Self { bases, cache: None }
          }
          pub(super) fn with_cache(mut self, cache: RefreshCache<'a>) -> Self {
              self.cache = Some(Box::new(cache));
              self
          }
          pub(super) fn build(self, root: &Path) -> Result<FileIndex, IndexBuilderError> {
              match self.cache {
                  None => Self::build_fresh(self.bases, root),
                  Some(cache) => Self::build_with_cache(self.bases, root, *cache),
              }
          }
          fn build_fresh(bases: Vec<FileBase>, root: &Path) -> Result<FileIndex, IndexBuilderError> { ... } // unchanged
          fn build_with_cache(bases: Vec<FileBase>, root: &Path, cache: RefreshCache<'a>) -> Result<FileIndex, IndexBuilderError> { ... } // see above
      }
      ```
      `from_scan` and `reuse_unchanged` are deleted — `new` takes an
      already-scanned `Vec<FileBase>` (pure data assembly, no I/O in the
      builder itself), and `with_cache` takes an already-constructed
      `RefreshCache` (built via `RefreshCache::load`, from ticket 14) as
      one argument, not four loose parameters.
- [x] `build_fresh`'s doc comment gets one line added: why it can't be
      re-expressed as `build_with_cache` against a synthetic empty cache
      — cross-reference `IndexDelta`'s doc comment (ticket 14, Design
      Decision 17) rather than re-explaining it inline. `build_fresh`
      never opens `IndexStore` at all, by design; forcing it through
      `RefreshCache` would both cost a needless store-open on every
      first-time build and — the actual hazard — risk conflating "no
      previous state to check" with "verified nothing was deleted,"
      which only `RefreshCache::load`'s real query can honestly claim.
- [x] `scan_root` (`src/index/scan.rs`) moves into `service.rs` as a
      private method: `IndexerService::scan(&self) -> Result<Vec<FileBase>,
      IndexBuilderError>`, reading `self.root` instead of taking a `root:
      &Path` parameter. `scan.rs` is deleted; its doc comment (sort
      invariant, `.git`/symlink/index-db skip rules) and its test module
      move into `service.rs`'s existing `#[cfg(test)] mod tests`. `mod.rs`
      drops `mod scan;`.
- [x] `IndexerService::build`/`refresh` are rewritten to the final shape:
      ```rust
      pub fn build(&self) -> Result<FileIndex, IndexError> {
          let bases = self.scan()?;
          Ok(builder::IndexBuilder::new(bases).build(&self.root)?)
      }

      pub fn refresh(&self) -> Result<FileIndex, IndexError> {
          let store = IndexStore::open(&self.root)?;
          let bases = self.scan()?;
          let index = {
              let read_txn = store.begin_read()?;
              let cache = builder::RefreshCache::load(&store, &read_txn)?;
              builder::IndexBuilder::new(bases).with_cache(cache).build(&self.root)?
          };
          if let Err(source) = store.persist_index(&index) {
              tracing::warn!(%source, "failed to persist refreshed index");
          }
          Ok(index)
      }
      ```
      This supersedes ticket 14's own `refresh()` snippet (which uses the
      interim `from_scan`/`reuse_unchanged` names) — ticket 14 lands that
      interim shape first for its own borrowing/phase-split/persist-on-
      refresh changes; this ticket updates the method names and moves
      scanning out, on top of it, not instead of it.
- [x] The two now-obsolete `reconcile_note` unit tests
      (`consumes_the_matched_previous_entry_so_it_is_not_double_counted`,
      `consumes_the_matched_previous_entry_even_when_the_record_changed`)
      are deleted — they assert an invariant ("the matched previous entry
      must be consumed from `prev_iter`, or a later call double-counts it
      as deleted") that becomes structurally impossible once
      `reconcile_note` never touches `prev_iter`/`previous` at all.
      Replaced with two simpler tests on `RefreshCache::reconcile_note`
      directly: an unchanged Note (`is_upserted: false`) reuses via point
      lookup, not reparse; a changed-or-new Note (`is_upserted: true`)
      reparses, not reuse.
- [x] A regression test covers an upserted **non-Note** file sitting
      between two Note-format bases in path order — proves the
      upserted-pointer consumption is unconditional (checked for every
      base) rather than only for Note-format ones.
- [x] `clippy::large_stack_frames` — re-run `cargo clippy` against the
      fully implemented result of both tickets (not this ticket in
      isolation) and treat every reported frame as new information. The
      original single-file investigation (ticket 14, Design Decision 12)
      found `reconcile_note` at 4615 bytes purely from its old
      `(Note, bool)` return plus `prev_iter`/`previous_matches_path`/
      `unchanged` dual-check logic — both of which are gone in this
      ticket's redesign — so the warning is expected to disappear, but
      confirm against the actual code, not the old measurement, since
      `reconcile_note` now also carries the backdating point-lookup and
      target-comparison logic, which didn't exist when 4615 bytes was
      measured.

## Comments

> *Filed after a full grilling session on this architecture-review
> candidate — decisions below are the user's confirmed answers, not
> proposals awaiting review.*

### Design Decisions (settled)

1. **Fold `has_deleted_note` into `diff_bases`, one pass** — `diff_bases`
   already visits every `previous`-only (deleted) entry during its merge,
   with `FileBase::format()` available at zero extra cost.
2. **`diff_bases`'s third return value is narrowly `has_deleted_note`, not
   a general staleness flag** — a deliberate departure from this ticket's
   first draft, which had `diff_bases` compute full staleness (both
   deleted *and* upserted Notes counting). Backdating (Design Decision 6
   below) means an upserted Note's contribution to staleness can only be
   known after comparing its outlinks, which needs Note content —
   information `diff_bases` (operating on `FileBase` alone) structurally
   doesn't have. Deletion staleness stays in `diff_bases` because it
   genuinely doesn't need content: a deleted Note always forces a
   recompute, full stop, no comparison possible or necessary.
3. **`RefreshCache` owns behavior, not just fields** — traced actual field
   usage before deciding: `previous` is read once (by `diff_bases`, at
   the top), `inlinks` once (by `diff_links`, conditionally, at the
   bottom), `store`/`read_txn` throughout the whole loop. Three different
   lifetimes of relevance bundled into one struct that `build_with_reuse`
   (this ticket's `build_with_cache`) immediately unpacked anyway was the
   actual shallow-module smell — not the struct's existence, but
   `IndexBuilder` reaching into its fields directly instead of asking it
   to do things.
4. **`delta.rs`'s `diff_bases`/`diff_inlinks` stay pure free functions;
   `RefreshCache`'s methods are thin wrappers** — considered giving
   `RefreshCache` the diffing logic directly (no separate `delta.rs`
   functions); rejected, because `delta.rs`'s functions are independently
   unit-testable against explicit `(current, previous)` pairs with no
   `IndexStore`/`ReadTransaction` fixture required, and `RefreshCache`'s
   methods are the ergonomic binding for the one caller that already
   holds "previous" as its own state. Two real usage shapes, not one
   function pretending to be two.
5. **`parse_note` stays a free function, not a `RefreshCache` method** —
   it has two real callers (`build_fresh`, which has no `RefreshCache`,
   and `RefreshCache::reconcile_note`) — the one case in this redesign
   where a shared free function is correct instead of either type owning
   it outright.
6. **Backdating compares target sets, not full `Link`/`Note` equality** —
   `Link::text` (display text) is irrelevant to the inlink graph;
   comparing it would force unnecessary recomputes on the common case of
   a user editing a link's visible label without moving its target.
7. **Backdating fails open on lookup error** — it's a pure optimization
   layered on top of correctness that doesn't depend on it (the fresh
   Note is always successfully parsed regardless); a corrupted previous
   row should degrade to "assume changed, recompute," never to a failed
   `refresh()`.
8. **`IndexBuilder::new`/`with_cache`/`build` over free functions** — an
   earlier proposal in this same grilling session collapsed `IndexBuilder`
   into two free functions entirely, reasoning that every call site
   chains `from_scan(...).reuse_unchanged(...).build(...)` immediately
   with no caller ever holding partial state. Overridden: a proper
   builder (`new`/`with_cache`/`build`) is the more idiomatic shape for
   "assemble optional configuration, then produce one thing," and
   separating `new` (pure data assembly) from the old `from_scan`
   (assembly *and* I/O in one step) is what actually made the original
   shape feel like ceremony — not the presence of a builder struct itself.
9. **Scanning moves to `IndexerService::scan`, not merged wholesale into
   `service.rs` as inline code without its own identity** — `scan.rs`'s
   *file* is deleted (its function becomes a private method reading
   `self.root`), but its doc comment (sort invariant, skip rules) and
   test module move with it, preserved as a cohesive unit inside
   `service.rs`, not scattered.
10. **`IndexStore` embedding in `IndexerService` — considered again in
    this ticket's context, still rejected** — see ticket 14's Design
    Decision 16. `RefreshCache::load` (ticket 14) already gets
    `IndexerService::refresh` down to one `IndexStore::open()` per call;
    embedding would only additionally help a scenario (one service
    instance reused across many refreshes) nothing in this codebase does
    today.
11. **`IndexBuilder`'s stored `RefreshCache` field is named `cache`, not
    `reuse`** — renamed alongside `with_cache`/`build_with_cache`, since
    it's the same value throughout: the struct field, the `with_cache`
    parameter, and `build_with_cache`'s parameter all refer to the same
    `RefreshCache`, and naming it differently at each point (field
    `cache`, but a local destructured reference still called `reuse`)
    would read as inconsistent rather than intentional. The field itself
    is renamed in ticket 14, since it's part of that ticket's `RefreshCache`
    field/ownership reshape; this ticket's `build_with_cache` and
    `with_cache` continue that naming rather than reintroducing `reuse`.

## Agent Brief

**Category:** enhancement

**Summary:** Collapse `IndexBuilder::build_with_reuse`'s two independent
merges over `(bases, cache.previous)` into one; move that merge's
"what changed" logic fully into `RefreshCache`'s own methods instead of
`IndexBuilder` reaching into its fields; backdate staleness to actual
outlink changes instead of any Note edit; and reshape `IndexBuilder` into
a proper `new`/`with_cache`/`build` builder (renaming `build_with_reuse`
to `build_with_cache`) with scanning moved out to `IndexerService::scan`.

**Current behavior (after ticket 14 lands):** `RefreshCache`'s fields are
borrowed, boxed, and constructed only via `RefreshCache::load`; its
field on `IndexBuilder` is named `cache`. `IndexBuilder::build_with_reuse`
still calls `diff_bases(&bases, &cache.previous)` for the persistence
delta, then separately walks `bases` alongside a fresh `prev_iter` over
`cache.previous` via `has_deleted_note`+`reconcile_note` to decide
reparse-vs-reuse and assemble `dirty` — both walks compare the same
`FileBase` equality with nothing enforcing they agree except that they
happen to today. `dirty` is set unconditionally for every reparsed Note
regardless of whether its outlinks actually changed. `IndexBuilder` is
constructed via `from_scan(root)` (which scans internally) and
`reuse_unchanged(cache)`.

**Desired behavior:** see the checklist above — this section intentionally
doesn't repeat it.

**Key interfaces:**

- `delta.rs` (new) — `IndexDelta`, `IncrementalDelta` (+`is_empty`),
  `diff_bases` (3-tuple, `has_deleted_note` not general staleness),
  `diff_inlinks`.
- `RefreshCache` (`src/index/builder.rs`) — gains `diff_bases`,
  `reconcile_note` (with backdating), `diff_links`, `into_inlinks`
  methods. Fields/`load` constructor/`cache` field name already land via
  ticket 14.
- `IndexBuilder` (`src/index/builder.rs`) — `new`/`with_cache`/`build`,
  replacing `from_scan`/`reuse_unchanged`/`build`; `build_with_reuse`
  renamed `build_with_cache`. Loses its own `reconcile_note` entirely.
- `IndexerService` (`src/index/service.rs`) — gains private `scan`;
  `build`/`refresh` updated to the final call shape above.
- `scan.rs` — deleted; `mod.rs` drops `mod scan;`, gains `mod delta;`.

**Out of scope:**

- Ticket 14's redb-adapter/persist-on-refresh/`RefreshCache`
  field-and-constructor work — assumed already landed.
- Ticket 16's `LINKS` reconstruction fidelity.
- Embedding `IndexStore` in `IndexerService` — see Design Decision 10.

## Implementation details

Implemented on branch `one-merge-diff-not-two` (branched from
`d4d64f7`, ticket 14's integration commit) in three commits:

- `2891575` — `feat(index): rebuild builder as cache-driven, backdate staleness`
- `5699c3b` — `docs(index): comprehensive doc revision for crates.io publication`
- `37cf3b9` — `fix(index): restore diff_files/diff_inlinks as pure functions in delta.rs`

### Delivered shape

- `src/index/delta.rs` — `IndexDelta`, `IncrementalDelta` (+`is_empty`),
  and the two-pointer merge functions, unit-tested against plain
  `(current, previous)` values with no `IndexStore`/`ReadTransaction`
  fixture.
- **Deviation from the checklist's literal naming:** the merge-join
  function is `diff_files`, not `diff_bases` as written throughout this
  ticket's checklist — renamed at the user's explicit direction during
  review, after the initial implementation shipped it as `diff_bases`.
  `diff_inlinks` keeps its checklist name (no rename requested there).
  Its return shape matches the checklist exactly: `(Vec<PathBuf>,
  Vec<PathBuf>, bool)`, third value `has_deleted_note`.
- `src/index/cache.rs` (new file, not `builder.rs` as the checklist's
  code samples show) — `RefreshCache` and its methods
  (`diff_files`/`reconcile_note`/`diff_links`/`into_inlinks`) moved
  here as a deliberate deepening beyond the approved plan: it had grown
  to 5 methods + 2 helpers, and `builder.rs` was accumulating structs
  it doesn't own. `diff_files`/`diff_links` are one-line delegations to
  `delta.rs`'s free functions (fixed post-review; the initial pass had
  duplicated the merge logic inline in `cache.rs` instead of
  delegating, diverging from Design Decision 4). The `is_upserted: bool`
  decision that `reconcile_note` and `diff_files` communicate is a
  `NoteCacheState` enum (`Stale`/`Fresh`), not a bare `bool` — named
  through an interactive naming pass during review (rejected
  `FileStatus`/`ReconcileStrategy` as less specific).
- `src/index/builder.rs` — `IndexBuilder<'a>` is the checklist's
  `new`/`with_cache`/`build` builder; `build_with_reuse` renamed
  `build_with_cache`; own `reconcile_note` deleted; `from_scan`/
  `reuse_unchanged` deleted. Stale `from_scan_*` test names renamed to
  `new_*` (fixed post-review — leftover names referencing a deleted
  API).
- `src/index/service.rs` — `scan_root` moved in as private
  `IndexerService::scan`, `scan.rs` deleted, `mod.rs` drops `mod scan;`
  and gains `mod delta;`/`mod cache;`. `refresh()`'s doc comment fixed
  post-review: it claimed inlinks recompute "whenever a Note's content
  or metadata changed," which backdating makes false — corrected to
  "whenever a Note is added/removed, or a changed Note's outlink
  targets actually differ from its previously-persisted value."
- Backdating: `reconcile_note`'s reparse branch point-looks-up the
  previous Note, compares deduplicated sorted outlink-target sets (not
  full `Link`/`Note` equality), fails open (logs via
  `tracing::debug!`, treats as "changed") on any lookup error other
  than not-found.
- Every test the checklist named is present: `diff_files`/`diff_inlinks`
  unit tests in `delta.rs`; the two simplified `reconcile_note` tests
  (unchanged reuses via point lookup, changed-or-new reparses)
  replacing the two now-impossible double-counting tests; the
  upserted-non-Note-between-two-Notes regression test in `builder.rs`;
  and all five backdating signal tests in `service.rs`
  (`refresh_persists_so_a_fresh_load_reflects_the_change`,
  outlinks-changed, outlinks-reordered-still-backdates,
  brand-new-note-never-backdated, corrupted-previous-row-fails-open).

### Verification

- `cargo fmt --check`: clean.
- `cargo clippy --workspace --all-targets --all-features -- -D
  warnings`: fails with the same 11 pre-existing errors as `main`
  before this ticket (none in `src/index/`), confirmed identical by
  diffing the error set against the pre-existing baseline.
- `cargo test --lib index::`: 132 passed, 0 failed.
- `cargo doc --no-deps --lib`: zero warnings.
- `cargo test --doc`: 13 passed.

### Corrections (discovered during review, applied before merge)

A dedicated `rust-code-review` pass against the approved plan and this
ticket's checklist found six issues in the initial implementation
(`2891575`), fixed in `37cf3b9`:

1. `delta.rs`'s `diff_files`/`diff_inlinks` were missing entirely —
   `RefreshCache::diff_files`/`diff_links` had the two-pointer merge
   inlined directly instead of delegating, contradicting Design
   Decision 4. Restored as pure, fixture-free functions with their own
   tests moved from `cache.rs`.
2. `RefreshCache::diff_files`/`diff_links` rewritten as one-line
   delegations once (1) landed.
3. `service.rs`'s `refresh()` doc comment overclaimed inlink recompute
   conditions (see Delivered shape above).
4. A stale test comment in `resolves_stale_ambiguous_wikilink_after_unrelated_deletion`
   still described the pre-backdating "gated on whether anything
   changed" behavior.
5. `builder.rs`'s `from_scan_produces_sorted_records`/
   `from_scan_parses_markdown_notes` test names referenced the deleted
   `from_scan` API; renamed to `new_produces_sorted_records`/
   `new_parses_markdown_notes`.
6. `NoteCacheState` derived unused `PartialEq`/`Eq`; trimmed to
   `Clone`, `Copy`, `Debug`.

`RefreshCache`'s file relocation to `cache.rs` (a net improvement, not
finding 1's original review scope) and the `diff_bases`→`diff_files`
rename were both separate, explicit user calls made during the same
review session — not silent scope drift.
