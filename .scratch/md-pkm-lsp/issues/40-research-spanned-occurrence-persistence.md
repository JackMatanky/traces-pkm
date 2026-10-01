# Research: occurrence-span persistence — verified mechanics of carrying `Spanned<T>` spans across the redb boundary vs keeping them transient

Type: research
Blocked by: 11, 39
Status: resolved

## Question

Note-parse ticket 04 originally designed occurrence spans as a mandatory `Spanned<T> { value: T, span: ByteSpan }` on outlinks, body-tag occurrences, and inline-field entries, with `#[serde(skip)]` on the span plus a doc invariant: "spans loaded from the store decode as 0..0 and must never be trusted." The user withdrew that design (option A) mid-triage — a mandatory field that is structurally always present but semantically dead after every store load is a type-system lie, with correctness resting on prose nothing enforces.

What remains is a sharp but deliberately un-decided question: **how do occurrence spans cross — or fail to cross — the persistence boundary?** Record the verified mechanics and blast radius so the eventual HITL decision isn't re-derived from scratch:

- **B — persist spans**: plain serde derives on `Spanned<T>`; spans round-trip through the `NOTES` table; requires amending ticket 11 §8 ("spans stay transient, never in redb").
- **C — honest absence**: `span: Option<ByteSpan>`; absent after a store load; keeps §8; costs `Option` plumbing at consumers and equality/ordering rework.

Investigate and record, with exact file/line and version citations:

1. `Spanned<T>`'s current state: derives, equality/ordering semantics, serde absence, its existing production users (are any anywhere near the persist path?), and exactly which tests pin its equality/ordering behaviour.
2. Serde mechanics against the pinned versions (`serde` 1.0.229, `postcard` 1.1.3, `redb` 4.2.0 — `Cargo.lock`): what `skip` does on both serialize and deserialize, what a skipped field decodes to, whether that requires `T: Default`, where the skip attribute must live for a transparent wrapper (inside `Spanned` vs on a `Note` field), and what each does to the wire layout.
3. postcard + redb reality: is the format schema-aware (field names/counts) or positional? What happens to pre-existing rows when a persisted field is added? Is there any versioning hook? (Pre-release: the project has ruled migration machinery a non-concern — record the mechanics anyway as cost evidence, not as a blocker.)
4. Test blast radius: which existing tests pass or fail under A, B, and C — the postcard round-trip tests that compare whole `Note`s, the two `Spanned` ordering pins, and equality-based parser tests.
5. Ticket 11 §8's stated rationale, quoted, with each cost claim marked still-valid vs invalidated pre-release.
6. The settling evidence: whether any real consumer needs byte ranges from *store-assembled* `Note`s (redb-loaded, no live parse) or only from live/re-parsed buffers — what downstream open tickets 26 (definition/references/hover/rename) and 27 (structural symbols/editor intelligence) actually demand, and what tickets 13/14's resolved answers say about serving closed-file requests.

Hard constraints (on the research, not on the eventual decision, which stays HITL):

- No production code written or modified.
- Does not decide B vs C — that is grilling ticket 41; this ticket's Answer presents evidence, a lean, and the open questions.
- Every repo claim cites exact file paths/line numbers; every crate claim verified against the exact `Cargo.lock` version via rust-docs/source, not memory.

Read-first: `.scratch/md-pkm-lsp/map.md`, ticket 11 (§1, §8), `research/39-source-span-position-model.md` (persistence findings), note-parse tickets 01 and 04, `docs/agents/issue-tracker.md`. Inspect directly: `src/position.rs`, `src/note/model.rs`, `src/note/parser.rs`, `src/note/parser/task.rs`, `src/note/parser/lexer.rs`, `src/query/grammar/`, `src/index/store.rs`, `src/index/tables.rs`.

Output: `.scratch/md-pkm-lsp/research/40-spanned-occurrence-persistence.md`.

## Answer

**Option A (mandatory span + `serde(skip)` + the "loaded spans decode 0..0" prose invariant) stays withdrawn. B and C are both mechanically viable, and the choice is evidence-shaped, not taste-shaped: B is the only option with zero test churn — spans persist, so the derived span-inclusive `Eq`/`Ord` stay honest and every round-trip assert and both ordering pins pass unmodified — but it requires amending ticket 11 §8. C keeps §8 but forces value-only manual equality (a decoded `None` would otherwise fail the whole-`Note` `assert_eq!(decoded, note)` round-trips), which then contradicts the span-inclusive ordering the two pins pin — the same test churn A carried, now with honest `Option` semantics — plus `Option` plumbing at every span consumer.** The decision is deliberately deferred to grilling ticket 41 (blocked on tickets 26/27, the consumers that would actually read ranges, plus a re-read of 13/14's resolved answers for closed-file serving mode). Current lean: **B** — pre-release removes §8's format-bump cost, persisted spans are exactly as fresh as the rest of the row (tags/outlinks are already snapshot data refreshed together), and no store-side span consumer exists today to be hurt.

Full mechanics, exact file/line citations, per-test blast radius, §8's cost claims reviewed one by one, and the settling-evidence map: [spanned-occurrence-persistence.md](../research/40-spanned-occurrence-persistence.md).
