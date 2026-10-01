# Research: occurrence-span persistence — `Spanned<T>` across the redb boundary (persist vs transient)

Resolves ticket [40-research-spanned-occurrence-persistence](../issues/40-research-spanned-occurrence-persistence.md).
Feeds: note-parse ticket 04 (spans removed from its scope), grilling ticket 41 (the B-vs-C decision), ticket 11 §8 (possible amendment).

## Executive summary

Option A — mandatory `span: ByteSpan` + `#[serde(skip)]` + the prose invariant "store-loaded spans decode 0..0, never trust them" — stays withdrawn: the type says "always present," the store says "always dead," and only a comment knows the difference. Two mechanically-verified options remain:

- **B — persist spans**: add `Serialize, Deserialize` to `Spanned<T>`'s derive list. Spans round-trip through the `NOTES` table, the derived span-inclusive `Eq`/`Hash`/`Ord` stay honest, and **every existing round-trip assert and both ordering pins pass unmodified**. Requires amending ticket 11 §8 ("spans stay transient, never in redb").
- **C — honest absence**: `span: Option<ByteSpan>`, `#[serde(skip)]`, decodes to `None`. Keeps §8, but a decoded `None` fails the whole-`Note` `assert_eq!(decoded, note)` round-trips, forcing manual value-only `PartialEq`/`Eq`/`Hash` — which then contradicts the span-inclusive `Ord` the two pins pin, so ordering goes value-only too and **both pins are rewritten** (the same test churn A carried), plus `Option` plumbing at every span consumer.

Neither option touches today's wire: `Spanned` has no serde today and no `Spanned` lives inside a persisted `Note` yet. B and C shape the deferred follow-up ticket that would put `Spanned<Link>`/`Spanned<Tag>`/`Spanned<NoteFieldValue>` into `Note`'s collections.

**The decision is evidence-shaped**: it turns on whether any consumer ever needs byte ranges from *store-assembled* `Note`s (redb-loaded, no live parse) or whether every range-serving path re-parses the file first — ticket 11 §8's load-bearing claim, and exactly what open tickets 26/27 will say. **Current lean: B**, because pre-release removes §8's format-bump cost, persisted spans are exactly as fresh as the rest of the row, and no store-side span consumer exists today. Not decided — grilling ticket 41.

## Where the design stands: A withdrawn, B and C open

The original note-parse 04 design: `Spanned<T> { value: T, span: ByteSpan }`, span mandatory, `#[serde(skip)]` on the span (serde fills skipped fields with `Default::default()`; `ByteSpan` derives `Default`), manual value-only equality so round-trips compare equal, and a doc invariant "spans loaded from the store decode as 0..0." Withdrawn by the user mid-triage (2026-10-01 session): a mandatory span that is semantically dead after every load is a correctness-by-prose lie across the persist boundary — a caller reading the type sees "always present" with no way to know it lies.

What A's withdrawal actually decides, and what it doesn't:

- Decided: prose invariants that paper over a type/representation mismatch are out.
- Not decided: persist spans (B) vs model their absence (C). Both are honest; they optimize different things.

## Verified mechanics: `Spanned<T>` today

All line numbers from the 2026-10-01 tree (post `f9d0103b`/`6fef836f`/`57d6dd0d`).

- **Struct and derives** — `src/position.rs:411-416`: `#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)] pub(crate) struct Spanned<T> { value: T, span: ByteSpan }`. **No serde derives at all** — the type cannot cross the persist boundary today. `Eq`/`Hash`/`PartialEq` are span-inclusive (both fields). The `Default` derive bounds `Spanned<T>: Default` on `T: Default` — separate from, and not required by, the skip mechanics below.
- **Ordering** — manual `PartialOrd` (`src/position.rs:516-524`) and `Ord` (`:526-531`): compare value first, span as tiebreaker.
- **Pinned semantics** — `mod spanned` tests: `orders_by_value_then_span_start_then_span_end` (`src/position.rs:853`) and `keeps_a_zero_length_span_ordered_by_its_position` (`:863`) pin value-then-span ordering; `carries_its_value_with_its_span` (`:846`), `provides_span_bounds_accessors` (`:872`), `decomposes_into_parts_and_borrows_as_ref` (`:882`) pin the accessors. These two ordering pins are the only tests that die if ordering goes value-only.
- **Production users — all transient, none near the persist path**: the task-shorthand token stream (`src/note/parser/task.rs:22`, built at `src/note/parser/lexer.rs:43-49` via `Spanned::from_usize_range`) and the query grammar's token streams (`src/query/grammar/source.rs`, `expr.rs:156`, `filter.rs:51-66`; re-exported at `src/lib.rs:132`). A session-wide search found **no production collection or comparison that depends on span-inclusive equality/ordering** — no `Spanned` in any map, set, or sort; span *reads* (`.span_usize()`, `.start_usize()`) exist, span-inclusive *equality/ordering* semantics are consumed only by the tests above.
- **Supporting types**: `ByteSpan` derives full serde *and* `Default` (`src/position.rs:72-89`, struct at `:86`) — so a skipped `span` field initializes to `0..0` without touching `T`. `BytePos(u32)` likewise (`:195-204`). `SourceLine` has hand-rolled serde — serialize as plain `u32`, deserialize rejecting zero (`src/position.rs:389-409`) — the precedent for position-bearing fields crossing the store.

## Serde mechanics (verified against `serde` 1.0.229 / `postcard` 1.1.3 / `redb` 4.2.0, `Cargo.lock`)

1. **`#[serde(skip)]`** = `skip_serializing` + `skip_deserializing`. On deserialize the field is initialized with `Default::default()` of **the field's type** — skipping `span` inside `Spanned<T>` needs `ByteSpan: Default` (✓), **not** `T: Default`. Serialization and deserialization skip symmetrically, so both directions agree on layout for any given struct definition.
2. **The skip must live inside `Spanned`**, not on a `Note` field. `Note`'s collections (`outlinks`, tags, inline fields) are *value-bearing* — a skip at the `Note`-field level would drop the values themselves, not their spans. Because `Spanned` would be serialized as a transparent wrapper (its only serialized member being `value`), the wire form of `Box<[Spanned<Link>]>` equals the wire form of `Box<[Link]>`: the wrapper is invisible on the wire, spans simply present or absent.
3. **postcard is schema-less and positional.** Structs serialize as their field values in declaration order — no field names, no field count. Consequences:
   - Adding `skip` to a field that previously serialized (not today's case — `Spanned` has no serde — but the shape of the decision) is layout-symmetric for any uniform reader/writer pair.
   - **Adding a newly-persisted field breaks rows written before it**: the positional decoder reads whatever bytes follow as the new value, with no names/counts to detect the mismatch; `#[serde(default)]` cannot rescue this (a positional format never reports a field as absent). Old-writer rows read by *older* code with *trailing* extra fields can work, since `from_bytes` ignores trailing bytes — but that asymmetry does not help new code reading old rows, and does not apply at all to spans nested inside existing collection elements (mid-stream layout shift either way).
   - This is why ticket 04's `headings` field and any B-style span bytes both mean "old rows don't decode; rebuild" — pre-release, a routine rebuild, no version machinery (explicit project direction).
4. **redb stores opaque bytes.** The schema is seven `TableSpec` entries (`src/index/tables.rs:92`); there is no per-row version column, no decode hook, no migration facility. redb neither detects nor mitigates an encode-format change — all format risk lives in the serde layer above it.

## Option B — persist spans

**Mechanics**: add `Serialize, Deserialize` to `Spanned<T>`'s derive list (`src/position.rs:412`) with the usual `T: Serialize`/`T: Deserialize` bounds — `Link`, `Tag`, and `NoteFieldValue` all derive serde already. Nothing else in the type changes; span-inclusive `Eq`/`Hash`/`PartialOrd`/`Ord` remain derived/manual exactly as today.

**Test blast radius: zero.** Round-trips compare equal because spans genuinely survive encode→decode; the ordering pins test semantics that didn't change. The equality-sensitive round-trip asserts — `assert_eq!(decoded, note)` in `src/note/model.rs:548` (`:589`), plus `src/index/store.rs:1861`, `:1886`, `:2006` and `src/index/service.rs:420`, `:854` (nine postcard round-trip tests counted crate-wide this session; these six compare note/link-bearing structures) — all stay green unmodified.

**Wire cost**: 8 bytes per spanned occurrence (two `u32`s) inside `NOTES` rows; spans ride the same row snapshot as tags/outlinks, refreshed together on every persist (`PersistRequest::rebuild`/`incremental`, `src/index/store.rs:44-73`).

**Requires amending ticket 11 §8**, quoted in full: *"Spans are cheap to recompute (re-parsing a file the LSP just read also re-derives every span in the same pass), invalidating on every edit (poor cache-hit shape), and persisting them triggers an avoidable index-format version bump. Only live-editing-session requests (hover, completion, rename) need spans, and those are against a file whose current text the LSP already has to touch."*

Cost-claim review, pre-release:

| §8 claim | Status pre-release |
| --- | --- |
| "avoidable index-format version bump" | **Invalidated as a blocker** — project direction: migration machinery is a non-concern before release; the real cost is one rebuild of the `NOTES` table. |
| "invalidating on every edit (poor cache-hit shape)" | **Weakened** — persisted spans are exactly as (in)valid as every other persisted field of the same `Note` row; outlinks and tags are equally edit-stale and are persisted anyway. Refresh rewrites rows wholesale; spans are not uniquely stale, they ride the same snapshot. |
| "cheap to recompute" | **Still true, cuts both ways** — cheap to recompute is an argument for *not storing* only when the serving path actually re-parses; it is not a correctness argument either way. |
| "only live-session requests need spans; those touch current text anyway" | **The load-bearing claim, still open** — true only if every range consumer is served from a live/re-parsed buffer. That is precisely what tickets 26/27 (range consumers) and 13/14 (serving mode) determine. |

## Option C — honest absence (`span: Option<ByteSpan>`)

**Mechanics**: change the field to `Option<ByteSpan>`, `#[serde(skip)]` (or default), decodes to `None` — structurally, not prose, says "this span did not come from the store." Keeps §8 untouched.

**Test blast radius: the churn A carried.** A decoded `None` against an encoded `Some(..)` fails span-inclusive `PartialEq`, so the whole-`Note` round-trip asserts (`src/note/model.rs:589` and the store/service list above) fail until equality becomes value-only — three manual impls (`PartialEq`, `Eq`, `Hash` must move together to keep hash/equality consistent). Value-only `Eq` then violates the `Ord`↔`Eq` consistency contract against the current value-then-span `Ord` (`src/position.rs:526-531` — equal values must order `Equal`), so ordering goes value-only too, and the two pins at `:853`/`:863` fail and must be rewritten to assert value-only semantics. The alternative — keeping span-inclusive equality and teaching every round-trip test to normalize nested spans before comparing — is strictly more test churn for the same outcome.

**Plumbing cost**: every `.span()` reader handles `Option` (`Spanned` is read by the task lexer and query grammar today; future AST consumers join them). Note the reversal: "mandatory, never `Option`" was the original design's ruling — but that ruling's premise (a mandatory span that is *meaningful* everywhere, including after load) died with option A; C's `Option` marks a real representation difference instead of hiding one.

## The settling evidence

- **Store-assembled `Note`s are real and served today**: `StoreSnapshot = (SortedByPath<FileMeta>, SortedByPath<Note>, InlinkMap)` (`src/index/store.rs:38-39`) — the query side reads notes loaded from redb with no live parse. The only positions it consumes are *lines* (`resolve_list_ref`/`resolve_metadata_ref`, `src/query/results.rs:312`/`:297`, via `ListItem.line`) — the precedent that line-granular positions already cross the store boundary and get used.
- **No span consumer exists today** — production `Spanned` users are parser/query-internal and transient; nothing reads ranges off a store-loaded note.
- **The question for grilling 41**: do the range consumers in tickets [26 (definition/references/hover/rename)](../issues/26-definition-references-hover-rename.md) and [27 (structural symbols/editor intelligence)](../issues/27-structural-symbols-and-editor-intelligence.md) ever read ranges from store-assembled notes, or does every range-serving path re-parse the file first? [Research 39](39-source-span-position-model.md) already found the latter is the natural model (spans recomputed at request time from text the LSP has to touch anyway), and tickets 13/14's resolved answers need re-reading for closed-file serving mode at grilling time. If all serving re-parses, persisting spans is dead weight and §8's discipline wins (C, or simply never crossing the boundary); if any store-served path wants ranges, B is the only shape that works without honesty-denting `Option` plumbing.

## Recommendation

**Lean B, decide at grilling ticket 41 once 26/27 speak.** B is the only mechanically free option (zero test churn, derived semantics stay consistent), its wire cost is 8 bytes riding an already-snapshot row, and pre-release voids §8's format-bump argument. C becomes the right call if §8 is retained as a *principle* (spans are derived data — always recompute, never store) rather than a cost argument, or if 26/27 confirm every range consumer is re-parse-served. The deeper question — whether `Spanned` belongs in `Note`'s persisted collections at all vs. transient span views — remains ticket 11 §1's territory and is not reopened here.

## Open questions for grilling ticket 41

1. Does §8 survive as a principle ("spans are derived data") or fall as a cost argument (pre-release rebuilds are free)?
2. Do tickets 26/27's consumers need ranges from store-assembled notes, or only from live/re-parsed buffers? (Blocked on them; re-read 13/14's resolved answers for serving mode.)
3. If B: what replaces §8 — "persisted spans are valid for the snapshot they rode in, refreshed with every row write"?
4. If C: does the `Option` live inside `Spanned<T>`, or do `Note`'s persisted collections keep plain values with spans attached only in transient views?
