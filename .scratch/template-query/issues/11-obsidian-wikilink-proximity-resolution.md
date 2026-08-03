# 11 — Obsidian-Faithful Wikilink Ambiguity Resolution

**What to build:** Ambiguous wikilink stem matches resolve using Obsidian's nearest-path rule instead of giving up, backed by a zero-copy `BaseNameRef` type and a decision on whether `Note` should carry `FileRecord` data.

**Blocked by:** 10 — Derived Inlinks

**Category:** enhancement

**Status:** needs-triage

- [ ] Decide whether `Note` embeds or references its `FileRecord` (or just `folder`/`name`), or whether link resolution instead takes `&[FileRecord]` alongside `&[Note]`, so `resolve_target`/`find_unique_by_stem` can see folder placement without re-deriving it from `Path`.
- [ ] Add `BaseNameRef`, a borrowed counterpart to `BaseName` (mirroring the `&str`/`String` split), so stem comparisons in the resolution fallback path don't need an owned `BaseName` per candidate.
- [ ] An ambiguous wikilink stem match resolves via Obsidian's shortest-unique-path proximity rule (nearest common ancestor / fewest path segments from the linking Note) instead of unconditionally resolving to `None`.
- [ ] Ambiguity proximity itself cannot break (equal-distance candidates) still resolves to `None` rather than guessing.
- [ ] Existing `resolve_target`/`find_unique_by_stem` tests in `inlinks.rs` covering "ambiguous stem match resolves to None" are extended to cover proximity-resolved cases and the still-ambiguous equal-distance case.

## Comments

> This ticket was raised during code review of #10's `find_unique_by_stem`, which deliberately implements only exact-uniqueness stem matching (a `ponytail:` note at `inlinks.rs:72-74` already flags the gap). Filed to track the upgrade path rather than build it speculatively — no real vault has demonstrated a need yet.

### Discussion

- **Why `Note` doesn't already have this:** `derive_inlinks`/`resolve_target` (`inlinks.rs`) take `&[Note]` only; `Note` (`note/model.rs`) stores `path: PathBuf` but not folder-proximity-friendly data beyond that path. `FileRecord::name` (`index/file.rs:25`) is already a `BaseName`, but `FileRecord` and `Note` are separate parallel collections (`FileIndex::records`, `FileIndex::notes`) joined by path only where needed (`matched_pairs` in `mod.rs`). Obsidian's real algorithm needs to compare each ambiguous candidate's folder depth/shared-ancestor distance to the *linking* Note's folder, which isn't in scope for a function that only sees `&[Note]`.
- **Why not just call `BaseName::from` today:** `BaseName` stores an owned `String` and is built via a two-step fallible conversion (`file_name.rs` `TryFrom<&Path> for FileName` → `From<&FileName> for BaseName`). `find_unique_by_stem`'s O(n) fallback scan runs per unresolved wikilink outlink; allocating a `BaseName` per candidate per scan is wasted work when `Path::file_stem()` gives the same string borrowed. A `BaseNameRef` (borrowed, `Deref<Target = str>` or similar, mirroring `&str` next to `String`) would let this path and any future proximity comparison reuse the shared type without paying for an allocation it doesn't need.
- **Existing precedent for "ambiguous → error, don't guess":** `template/loader.rs::find_name_in` (`rejects_an_ambiguous_stem_match` test) already rejects ambiguous stem matches for template resolution rather than picking one. Any proximity rule added here should keep that same fallback: resolve when Obsidian's rule *can* disambiguate, still return `None`/error when it can't (e.g. two equally-nested candidates).

## Agent Brief

**Category:** enhancement
**Summary:** Extend derived-inlink wikilink resolution (`index/inlinks.rs`) to follow Obsidian's actual ambiguity tie-break (nearest file by path proximity to the linking Note) instead of only resolving stem matches that are already unique, and introduce a zero-copy `BaseNameRef` so the resolution path can reuse `file_name.rs`'s naming types without allocating per candidate.

**Current behavior:**
`resolve_target` (`index/inlinks.rs:75-93`) tries an exact path match, then a `.md`-appended path match, then falls back to `find_unique_by_stem` (`inlinks.rs:103-109`), which resolves only when exactly one indexed Note shares the wikilink's file stem; two or more candidates resolve to `None` even when Obsidian itself would pick the nearest one. `Note` (`note/model.rs`) has no folder-proximity data beyond its own `path`, and `FileRecord`'s already-computed `BaseName` (`index/file.rs:25`) is never available where wikilink resolution runs.

**Desired behavior:**
When a wikilink's stem matches more than one indexed Note, resolution picks the candidate Obsidian would: nearest by path proximity to the linking Note (shortest shared-ancestor distance), matching Obsidian's documented shortest-unique-path search. Genuine ties (no single nearest candidate) still resolve to `None` — this is a strictly more capable resolver, not a "guess when unsure" one. The comparison needs each candidate's folder placement relative to the linking Note, which requires either giving `Note` access to its `FileRecord`'s `folder`/`name`, or threading `&[FileRecord]` into the inlink derivation pass alongside `&[Note]` — pick one and justify it against the existing `matched_pairs`/parallel-collection design in `mod.rs` before implementing.

**Key interfaces:**
- `index/inlinks.rs` — `resolve_target`, `find_unique_by_stem`, and `derive_inlinks`'s doc comment (which currently states the O(n) stem-fallback cost and the ambiguous-resolves-to-`None` behavior; both need updating if the algorithm changes complexity or ambiguity handling).
- `file_name.rs` — add `BaseNameRef` alongside `FileName`/`BaseName`, following the module's existing pattern of distinct newtypes for distinct semantics (see the module doc's rationale for keeping `FileName`/`BaseName` separate).
- `index/mod.rs` — `FileIndex::records`/`FileIndex::notes` and `matched_pairs`, if the resolution path ends up needing both collections together.
- `index/file.rs` — `FileRecord::name`/`FileRecord::folder`, the data a proximity comparison would read.

**Out of scope:**
- Note-directory-relative resolution (`../sibling.md`) — a separate, already-flagged `ponytail:` gap in the same function, not part of this ticket unless naturally subsumed.
- Any change to outlink extraction (`note/links.rs`) or to Markdown-style (non-wikilink) link resolution, which already resolves unambiguously by path.
- Full Dataview/Obsidian parity beyond wikilink stem ambiguity (e.g. alias resolution, block references).
