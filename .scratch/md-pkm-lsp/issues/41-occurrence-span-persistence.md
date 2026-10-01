# Occurrence-span persistence: persist `Spanned<T>` spans through redb (amending ticket 11 §8), or keep them transient behind `Option<ByteSpan>`?

Type: grilling
Blocked by: 26, 27

## Question

Note-parse ticket 04's original span design — a mandatory `Spanned<T>` with `#[serde(skip)]` on the span plus the doc invariant "store-loaded spans decode 0..0, never trust them" — was withdrawn mid-triage as a type-system lie: correctness by prose across the persist boundary. Two mechanically-verified options remain, recorded with full evidence in [research 40](../research/40-spanned-occurrence-persistence.md):

- **B — persist spans**: plain serde derives on `Spanned<T>`; spans round-trip through the `NOTES` table; zero test churn (the derived span-inclusive `Eq`/`Ord` stay honest, every round-trip assert and both ordering pins pass unmodified); requires amending ticket 11 §8 ("spans stay transient, never in redb").
- **C — honest absence**: `span: Option<ByteSpan>`, `None` after a store load; keeps §8; forces manual value-only `PartialEq`/`Eq`/`Hash` (decoded `None` vs encoded `Some` fails the whole-`Note` round-trip asserts), which contradicts span-inclusive `Ord` — so ordering goes value-only and the two pins (`src/position.rs:853`, `:863`) are rewritten — plus `Option` plumbing at every span consumer.

Decide. The settling evidence: whether any real consumer needs byte ranges from *store-assembled* `Note`s (redb-loaded with no live parse — `StoreSnapshot` already hands `SortedByPath<Note>` to the query side, `src/index/store.rs:38-39`) or whether every range-serving path re-parses the file first (research 39's model: spans recomputed at request time from text the LSP already touches). That hinges on what this ticket's downstream consumers actually demand — [definition/references/hover/rename](26-definition-references-hover-rename.md) and [structural symbols/editor intelligence](27-structural-symbols-and-editor-intelligence.md) — and on what the resolved answers of [LSP persistence/caching](13-lsp-persistence-and-caching-strategy.md) and [live buffer vs filesystem overlay](14-live-buffer-vs-filesystem-overlay.md) say about closed-file serving; re-read those at grilling time.

Also in scope of the decision:

- If B wins: the replacement text for §8 (when are persisted spans trusted? — research 40's draft: valid for the snapshot they rode in, refreshed with every row write).
- If C wins: where the `Option` boundary sits — inside `Spanned<T>`, or plain values in `Note`'s persisted collections with spans attached only in transient views.

Feeds: note-parse ticket 04 (already rewritten Heading-only; spans return as a follow-up ticket once this decides) and md-pkm-lsp ticket 11 §1/§8.
