# 15 — One Merge-Diff, Not Two, in IndexBuilder::build_with_reuse

**What to build:** `IndexBuilder::build_with_reuse` currently walks
`(bases, reuse.previous)` twice, independently: once in `diff_bases`
(producing the `upserted`/`deleted` persistence delta), and again via
`has_deleted_note`+`reconcile_note` sharing a fresh `prev_iter` (deciding
reparse-vs-reuse per Note and assembling the `dirty` flag). Fold both into
one pass: `diff_bases` returns `(upserted, deleted, stale)` — `stale`
(renamed from `dirty`) computed for free since the merge already visits
every deleted-or-upserted entry and can check `FileBase::format()` on
both sides. Note reconciliation then drives off `upserted` membership (a
small peekable pointer over `Vec<PathBuf>`) instead of re-walking the full
`reuse.previous` a second time.

**Related to:** architecture review of `src/index/` (this repo's own
codebase-design skill run). Independent of ticket 16 — different file,
different motivation. Ticket 14 now also touches `builder.rs` — it boxes
`IndexBuilder`'s `reuse` field (a `large_stack_frames` fix, unrelated to
this ticket's `diff_bases`/`reconcile_note` rewrite) and should land
first, per its own stated priority. No adaptation needed here either
way: `reuse.previous`/`&reuse.store`/`&reuse.read_txn` read identically
through `Box`'s `Deref`. Otherwise independent — different motivation
(locality/duplication, not persistence correctness or the redb
adapter), rated `Worth exploring` rather than `Strong` in the
originating report (mostly a maintainability win — `FileBase` equality
is cheap, so the perf gain from walking a smaller `upserted` set
instead of the full `previous` is real but modest at typical vault
sizes).

**Category:** enhancement

**Status:** ready-for-agent

- [ ] `diff_bases` returns `(Vec<PathBuf>, Vec<PathBuf>, bool)` —
      `upserted`, `deleted`, `stale` — computing `stale` within its
      existing merge pass (a path is stale-relevant iff it's Note-format
      and either deleted-from-previous or upserted-in-current).
- [ ] `has_deleted_note` is deleted entirely — its job is now folded into
      `diff_bases`.
- [ ] `reconcile_note` drops its `prev_iter` parameter; the
      reparse-vs-reuse decision is driven by checking `base.path()`
      against a peekable pointer over `diff_bases`'s `upserted` (walked
      alongside `bases` in `build_with_reuse`'s existing loop), never
      consulting `reuse.previous` again. **The upserted-membership check
      — and consuming a match — runs unconditionally for every `base`,
      Note or not, before the format-gated `continue`**, exactly
      mirroring how the old `has_deleted_note` ran unconditionally.
      Skipping the check for non-Note bases would stall the peekable
      pointer, misaligning it for later Note-format bases and silently
      reintroducing the exact double-counting bug class this ticket
      exists to eliminate. `reconcile_note` stays a named private helper
      (not inlined) — matches this file's convention of small, named,
      doc-commented private helpers (`parse_note`, `folder_distance`,
      `io_error`), and it's still two distinct fallible operations
      (point-lookup-with-two-error-paths vs. reparse) worth a name and
      independent testability.
- [ ] `build_with_reuse` no longer assembles `dirty`/`stale` piecemeal
      across three sites (per-base `has_deleted_note`, per-note
      `reparsed`, a trailing `prev_iter.any(...)` check) — it's a single
      value returned from `diff_bases`. `new_inlinks_if_dirty` renames to
      `new_inlinks_if_stale`.
- [ ] The two existing `reconcile_note` tests
      (`consumes_the_matched_previous_entry_so_it_is_not_double_counted`,
      `consumes_the_matched_previous_entry_even_when_the_record_changed`)
      are deleted — they assert an invariant ("the matched previous entry
      must be consumed from `prev_iter`, or a later call double-counts it
      as deleted") that becomes structurally impossible once
      `reconcile_note` stops touching `prev_iter`/`previous` at all.
- [ ] Replaced with two simpler tests asserting the actual observable
      behavior instead: an unchanged Note (not in `upserted`) reuses via
      point lookup, not reparse; a changed-or-new Note (in `upserted`)
      reparses, not reuse — neither test needs any iterator-consumption
      bookkeeping.
- [ ] `diff_bases`'s own tests (currently only exercised indirectly via
      `build_with_reuse`/`refresh()` integration tests — it has no direct
      unit tests today) gain direct coverage for the new `stale` output:
      a deleted Note sets `stale`; a deleted non-Note file does not; an
      upserted Note sets `stale`; an upserted non-Note file does not.
- [ ] A regression test covers an upserted **non-Note** file sitting
      between two Note-format bases in path order — proves the
      upserted-pointer consumption is unconditional (checked for every
      base) rather than only for Note-format ones, exactly the shape of
      bug the old `prev_iter`-consumption tests (now deleted) used to
      guard against under the previous design.
- [ ] `clippy::large_stack_frames` on `reconcile_note` (currently flagged
      at 4615 bytes, over the project's 4096-byte
      `stack-size-threshold`) is resolved — confirmed empirically by
      implementing this ticket's refactor in isolation and re-running
      clippy: the warning disappears entirely once `reconcile_note`
      drops the `(Note, bool)` tuple return (down to a bare `Note`) and
      the `prev_iter`/`previous_matches_path`/`unchanged` dual-check
      logic collapses to a single `if is_upserted` branch. Not a
      coincidence — re-run clippy after implementing to confirm it still
      holds against the real (not experimental) code.

## Comments

> *Filed after a full grilling session on this architecture-review
> candidate — decisions below are the user's confirmed answers, not
> proposals awaiting review.*

### Design Decisions (settled)

1. **Fold `has_deleted_note` into `diff_bases`, one pass, three outputs**
   — `diff_bases` already visits every `previous`-only (deleted) entry
   and every `current`-only-or-changed (upserted) entry during its merge,
   with `FileBase::format()` available on both sides at zero extra cost.
   Computing the staleness flag there means one function owns "what
   changed and does it matter," instead of three call sites in
   `build_with_reuse` each contributing a piece.
2. **`dirty` renamed to `stale`** — closer to the actual meaning: whether
   the *previous, cached* inlinks are out of date relative to the fresh
   note set and need recomputing. `new_inlinks_if_dirty` →
   `new_inlinks_if_stale` follows the same rename.
3. **Note reconciliation drives off `upserted` membership, never
   `reuse.previous`** — the actual fix. `reconcile_note`'s "unchanged"
   branch only ever needed a yes/no ("is this path in the changed set?")
   to decide point-lookup-reuse vs. reparse; it never needed to inspect
   `previous`'s own entries once that yes/no is known.
4. **Keep `reconcile_note` named, simplified signature** — considered
   inlining it into `build_with_reuse`'s loop now that it shrinks to a
   handful of lines; declined, since it's still two distinct fallible
   operations (a point lookup that can fail two ways, or a fresh parse)
   and this file's own convention already extracts comparably small
   logic into named, doc-commented private helpers.
5. **Delete, don't migrate, the two `prev_iter`-consumption tests** — both
   exist purely to guard against a bug class (an un-consumed iterator
   causing a later call to double-count a matched entry as deleted) that
   this refactor doesn't patch, it removes the possibility of: once
   `reconcile_note` never touches `prev_iter`, there is no iterator left
   to under- or over-consume. This is the deletion-test proof for the
   whole ticket — deleting the mechanism that made those tests necessary
   is exactly what "the complexity was accidental, not earned" looks
   like in practice.
6. **Upserted-pointer consumption must be unconditional — audit
   correction** — re-derivation during a second grilling pass confirmed
   `diff_bases`'s `upserted` and the old `reconcile_note`'s
   reparse-vs-reuse partition are provably the same set (both keyed on
   full `FileBase` (in)equality), so the refactor is sound in principle
   — but only if the peekable pointer over `upserted` is advanced past
   every matching path, Note or not, not just Note-format ones. The
   original checklist wording didn't say this explicitly. Skipping
   non-Note bases would stall the pointer and misalign it for later
   Note-format bases, silently reintroducing the exact bug class
   (un-consumed matched entries causing later false positives)
   `builder.rs`'s own doc comments already record having been bitten by
   once before.
7. **`large_stack_frames` on `reconcile_note` is a genuine byproduct of
   this design fix, verified empirically** — implemented the planned
   refactor in an isolated experiment (then reverted) and measured with
   `cargo clippy`: the function's reported stack frame was 4615 bytes
   before, absent from clippy's output entirely after. This wasn't
   assumed; a sibling investigation into the same lint elsewhere in
   `src/index/` found the *opposite* result for a structurally different
   function (`IndexStore::load_table`, where the same kind of extraction
   made the warning worse), so the fix here is reported as confirmed,
   not inferred from a general pattern.

## Agent Brief

**Category:** enhancement
**Summary:** Collapse `IndexBuilder::build_with_reuse`'s two independent
two-pointer merges over `(bases, reuse.previous)` into one, so "what
changed" has a single definition instead of being derived twice by
separately-maintained logic.

**Current behavior:** `build_with_reuse` calls `diff_bases(&bases,
&reuse.previous)` to compute the `upserted`/`deleted` persistence delta
via its own two-pointer merge. It then *separately* walks `bases`
alongside a fresh `prev_iter` over `reuse.previous`, via
`has_deleted_note` (advances `prev_iter` past deleted entries, flagging
`dirty` if any were Notes) interleaved with `reconcile_note` (checks
whether `prev_iter`'s peeked entry matches the current base unchanged, to
decide point-lookup-reuse vs. reparse). Both walks compare the same
`FileBase` equality; nothing enforces they agree except that both happen
to rely on the same derived `PartialEq` today. Two `reconcile_note` unit
tests exist solely to verify `prev_iter` is consumed correctly so a later
call doesn't double-count a matched entry as deleted — an invariant that
only exists because of the duplicate walk.

**Desired behavior:** see the checklist above — this section intentionally
doesn't repeat it.

**Key interfaces:**

- `IndexBuilder::diff_bases` (`src/index/builder.rs`) — return type
  changes from `(Vec<PathBuf>, Vec<PathBuf>)` to
  `(Vec<PathBuf>, Vec<PathBuf>, bool)`.
- `IndexBuilder::has_deleted_note` — deleted.
- `IndexBuilder::reconcile_note` — drops its `prev_iter` parameter.
- `IndexBuilder::build_with_reuse` — the orchestration loop; `dirty`
  local renamed `stale`, assembled from `diff_bases`'s third return value
  instead of three scattered `|=` sites.
- No change to `IndexerService`, `IndexStore`, or anything outside
  `builder.rs` — this ticket is scoped entirely to the reconciliation
  algorithm, not the pipeline around it.

**Out of scope:**

- Tickets 14 and 16 (different motivation — see "Related to" above; the
  narrow `builder.rs` overlap with ticket 14 is a non-conflicting field
  change, not shared scope).
- Any change to `IndexDelta`/`IncrementalDelta`'s shape — `upserted`/
  `deleted`/`links_upserted`/`links_deleted` are unaffected; only how
  `upserted`/`deleted`/staleness get computed changes.
- Any change to `derive_inlinks` or the inlink-resolution algorithm
  itself — only whether it gets called (via `stale`) changes, not what it
  does.
