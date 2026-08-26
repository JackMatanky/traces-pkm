# 16 — Correlate LINKS Byte Keys Against Loaded Notes Instead of Reconstructing Independently

**What to build:** `IndexStore::load_links` (and its callers
`load_all`/`load_bases_and_links_via`) stop reconstructing `PathBuf`
values independently from stored `LINKS` bytes, and instead correlate
each stored edge against an already-loaded `notes` list, using the
authoritative `Path` reference from that list rather than a value
rebuilt purely from redb bytes.

**Related to:** the `src/index/store.rs` redb-adapter deepening (ticket
14). Not blocked by it — the correlation problem exists today with
`&str` keys too — but the byte-key work is what surfaced it, and lands
the `str::from_utf8` + lossy-fallback stopgap this ticket supersedes.
Deliberately numbered and ordered last of the three tickets from this
architecture review — lowest priority, do after ticket 14 and ticket 15.

**Category:** enhancement

**Status:** done

- [x] `read_links`/`read_all`/`read_files_and_links_via` resolve each
      stored `LINKS` edge's target/source through a `resolve` closure.
      `read_all` correlates against the already-loaded `notes` list
      (matched by `path.as_os_str().as_encoded_bytes()`), reusing the
      authoritative `Note::path`; `read_files_and_links_via` keeps
      byte-reconstruction deliberately (correlating would force loading
      the `NOTES` table it exists to avoid, and its links only feed the
      refresh diff, never query output).
- [x] A stored edge whose target or source matches no currently-loaded
      Note (stale/orphaned) is dropped: an unresolved target drops its
      whole edge set, an unresolved source is skipped, and an entry left
      with no surviving sources is omitted. Covered by
      `read_all_drops_link_edges_with_no_matching_note`.
- [x] `FileBase` and `Note` persist paths through a target-specific serde
      codec: Unix raw bytes and Windows wide-character arrays. Non-Unicode
      paths round-trip byte-exactly through `persist` → `load`, including
      `IndexerService::load()` inlinks. No data migration was required because
      this project has not shipped a release.

## Comments

> *Filed during an architecture-review grilling session on `src/index/`,
> not standard AI triage.*

### Triage Notes

Surfaced while grilling the "deepen `IndexStore`'s redb adapter"
candidate: fixing `LINKS`' key collision risk via `&[u8]` keys still
leaves `load_links` reconstructing `PathBuf` values from raw stored
bytes with no companion struct to fall back on (unlike `FILES`/`NOTES`,
whose `path` field lives inside the postcard-encoded `FileBase`/`Note`
value and never needs reconstruction from the key). The user preferred
not to introduce `unsafe` for that reconstruction; ticket 14 lands a
safe `str::from_utf8` + `String::from_utf8_lossy` fallback that narrows
the blast radius (from "any path can collide" to "only a non-Unicode
path can mis-reconstruct on load") without eliminating it.

Traced the actual current impact: production callers (`cli`
list/table/task, the minijinja `query`/`tasks` namespace) only ever call
`refresh()`, whose final in-memory `FileIndex` always derives inlinks
fresh from `derive_inlinks(&notes)` — real `Path` refs, no
reconstruction. The loaded/reconstructed `LINKS` data only feeds
`diff_inlinks`'s "previous" side (deciding what to write in the
incremental delta), so today's worst case for a non-Unicode-named note
is a wastefully-rewritten link edge on every refresh, not a wrong query
result. The path where a stale reconstruction could actually surface in
query output — `IndexerService::load()` — isn't called by any production
code path today.

Deferred rather than folded into ticket 14 because it's a second,
narrower interface change (`load_links` needs `notes` loaded and passed
in before it runs, today it's independent) with a second edge case to
define (stale/orphaned edge with no match in current `notes`) — scope
growth on a ticket that already covers a value codec, byte keys, a
migration path, and durability tuning. Numbered and sequenced last of
the three architecture-review tickets for the same reason: lowest
priority, safe to defer indefinitely without blocking ticket 14 or
ticket 15.

**Update, after tickets 14/15 were rewritten in the same architecture-review
session:** ticket 14 renames the self-opening `load_bases_and_links` to
`load_bases_and_links_via` (taking an external read transaction — see its
Design Decision 14) and gives `RefreshCache::load` sole ownership of
calling it (ticket 14's checklist, not ticket 15's — ticket 15 only adds
`RefreshCache`'s own `diff_bases`/`reconcile_note`/`diff_links`/
`into_inlinks` methods on top of the already-landed `load` constructor).
References here are updated to the new name.
While fixing the reference, corrected a pre-existing inaccuracy in this
ticket's own "Key interfaces" section: it claimed `load_all` and
`load_bases_and_links` "both already load `bases`/`notes` in the same
call" — false even before the rename, since `load_bases_and_links`
(like its `_via` successor) only ever loaded `bases`+`links`, never
`notes`. See the corrected "Key interfaces" bullet for what this means
for this ticket's actual correlation design.

## Agent Brief

**Category:** enhancement

**Summary:** Make `LINKS` round-trip through persistence with
byte-exact fidelity for every indexed Note's path, including
non-Unicode filenames, by correlating stored edges against
already-loaded Notes instead of reconstructing `PathBuf` values
independently.

**Current behavior (after ticket 14 lands):** `FILES`/`NOTES` tables
key on `&[u8]` derived from `path.as_os_str().as_encoded_bytes()`, with
the authoritative `PathBuf` always sourced from the postcard-decoded
`FileBase`/`Note` value — never reconstructed from the key. `LINKS` has
no such companion value: both its key (target path) and value (source
path) are bare path bytes, so `load_links` must build a `PathBuf`
purely from stored bytes. The safe path (`str::from_utf8` succeeding)
is exact; the fallback (`String::from_utf8_lossy`) is not, for the rare
non-Unicode-filename case.

**Desired behavior:** `load_links` (or whichever call site assembles
the final `InlinkMap` after loading) resolves each stored edge's raw
bytes against the freshly-loaded `bases`/`notes` list — e.g. a
`HashMap<&[u8], &Path>` built once from `notes` (LINKS edges are always
between indexed Notes, never other File Records) — and uses the
matched, authoritative `Path` rather than any independent
reconstruction. Falls back to a defined behavior (drop the edge, most
likely — a link to a Note no longer in the index shouldn't be shown)
when no match exists, rather than producing an orphaned `PathBuf`.

**Key interfaces:**

- `IndexStore::load_links` — today `pub(super) fn load_links(&self,
  read_txn: &ReadTransaction, table: MultimapTableDefinition<...>) ->
  Result<InlinkMap, DbError>`, independent of `notes`. Needs either a
  `notes: &[Note]` parameter or to move to a call site that already has
  both loaded.
- `IndexStore::load_all` already loads `bases`/`notes`/`links` together
  — threading `notes` into a link-correlation step there is a call-order
  change, not a new data dependency. `IndexStore::load_bases_and_links_via`
  (introduced by ticket 14, replacing the old self-opening
  `load_bases_and_links`) is different: it deliberately loads `bases` and
  `links` only, *not* `notes` — that's the whole point of its existence
  (`RefreshCache::load`, ticket 14/15, avoids touching the comparatively
  heavy `NOTES` table when reconciliation only needs unchanged Notes via
  point lookup). Correlating its `LINKS` load against `notes` would mean
  either loading `notes` there too (defeating that avoidance) or
  accepting that `load_bases_and_links_via`'s `LINKS` data stays
  reconstruction-based while `load_all`'s becomes correlation-based —
  decide which explicitly rather than assuming both callers converge for
  free, which the ticket's first draft incorrectly assumed of the
  original (also notes-free) `load_bases_and_links`.
- `InlinkMap` (`HashMap<PathBuf, Vec<PathBuf>>`, `src/index/inlinks.rs`)
  — return shape is unchanged; only how its entries are constructed on
  load changes.

**Out of scope:**

- Any change to `derive_inlinks`'s in-memory computation
  (`src/index/inlinks.rs`) — it already builds edges from real `&Path`
  refs with no reconstruction step; this ticket only touches the
  load-from-disk path.
- Ticket 14's `Postcard<T>` codec, byte-key migration, and
  `Durability` split — assumed already landed.
- Any change to `FILES`/`NOTES` reconstruction — already correct (path
  lives in the decoded value, not the key).

### Resolution

Landed on branch `correlate-links-loaded-notes`. `read_links` gained a
`resolve: impl Fn(&[u8]) -> Option<PathBuf>` closure so the
correlation-vs-reconstruction choice lives in its two callers:
`read_all` builds a `HashMap<&[u8], &Path>` from the loaded `notes` and
resolves through it (dropping orphaned edges); `read_files_and_links_via`
passes `|bytes| Some(path_from_bytes(bytes))` to keep reconstruction and
avoid loading `NOTES`.

**Byte-exact non-Unicode round-trip (criteria 3/4) landed with a custom
record-path serde codec.** The codec serializes Unix `OsStr` bytes or Windows
wide characters without `unsafe`, so `FileBase`/`Note` values can persist the
same non-Unicode paths that LINKS stores as raw bytes. Tests cover record and
full `IndexerService::load()` inlink round trips, orphaned-edge dropping, and
the intentional refresh-path reconstruction divergence.
