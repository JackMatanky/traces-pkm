# 06: Template parity — both format dialects + durationformat

**What to build:** Template authors can format dates in the dialect their copied recipe uses and can render human-readable durations: `durationformat` produces "3 days, 4 hours"; common moment tokens (`YYYY MM DD HH mm ss`, `Do`/`S`, `dddd`/`ddd`/`MMM`/`MMMM`, bracket literals) work; an unsupported token fails with an error that **names the token**, says the input was treated as moment-dialect, and points at the supported list. Invalid-pattern detection exists once — at the template engine's private `format_with` (`src/template/engine/date.rs:304`), the sole remaining renderer (ticket 03's remediation deleted the core copies; `grep -n "fn format_with" src/date.rs` → no match).

**Blocked by:** 04 (amended 2026-10-05: 05 edge dropped — no 06 item consumes 05's output; 05 and 06 parallelize after 04; 03's duration parts and 01's display dialect ride transitively).

**Status:** ready-for-agent — D8 decided 2026-10-05 (delete `InvalidPattern`, translator unsupported-token becomes a render error); brief attached
**Category:** enhancement — parity features dominate; the dead `InvalidPattern` variant is hygiene riding along, not the point.

Skills: `rust-unit-testing`, `rust-integration-testing`, `rust-skills`, `rust-doc`. Rules: `type-numeric-fmt` + `type-display-vs-debug` (N19's exponent-threshold dialect governs only `DurationSeconds`'s `Display` fallback rendering; `durationformat` is the humanizer of parsed durations); the translator emits only a subset of chrono's documented strftime table (`chrono::format::strftime`, linked via intra-doc) and adds NO parallel specifier validator — invalid patterns are chrono's `DelayedFormat` error; `%+` is forbidden (chrono advises against it); docs note `%Z` = offset only, `%S` may be 60 (leap second), week tokens are `%V`/`%G`; doctests maintained (`cargo test --doc`); translator is a pure function at the template seam — no `Dialect` trait (review §5.7); strictness deliberate — never silently mistranslate an ambiguous token (D-b). Design record: `../review.md` §4 (D-b), §9 (S4+S8); spec stories 7–9.

- [ ] `durationformat` filter renders human-readable durations (B14), spec at review §3 Dataview L6803–6825 — compound unit-list display follows the humanizer convention catalogued in ../research/general-temporal-libraries.md (magnitude-based, e.g. pretty-ms/`humanize` precedents)
- [ ] **(rewritten 2026-10-05, §11 X-A)** Invalid-pattern rendering stays at exactly one site: the engine's private `format_with` (`src/template/engine/date.rs:304`) maps chrono's `DelayedFormat` and no second copy is introduced — the original N21 premise ("`format_with` has a live consumer (first Decision-C wiring); shared renderer surfaces chrono's invalid-pattern error once, call sites map their own types") is **obsolete**: Decision-C's three core `format_with` copies were deleted by ticket 03's remediation (dedup achieved by deletion, not by a shared `render_pattern`), so the work here is *confirmation + pin*: one site renders, call sites (`date.rs` filter/template paths) map errors to their own types, regression-pinned so a future copy doesn't reappear
- [ ] Format bindings per spec: documented grammar is `chrono::format::strftime` (intra-doc linked in touched rustdoc); `%+` is never emitted — pinned by test; invalid-pattern errors come from chrono's `DelayedFormat` only — no parallel specifier validator — with a test asserting an invalid pattern surfaces chrono's error mapped at the call site
- [ ] Format rustdoc notes `%Z` prints only an offset, `%S` may render 60 (leap second), and week numbers use `%V`/`%G` (ISO) — never `%U`/`%W`
- [ ] Moment-dialect translation scope 1 at the template seam: bracket literals, `YYYY MM DD HH mm ss` family, `Do`/`S`, `dddd`/`ddd`/`MMM`/`MMMM` (T4); the only strftime-grammar surface left is the engine's private `format_with` (template seam — the original "core `format_with` keeps strftime as its only grammar" line was written when a core copy existed; it no longer does)
- [ ] Unsupported moment token → error naming the offending token, identifying moment-dialect, listing where supported tokens are documented — as a template-render error at the engine seam, not a `DateError` (D8 decided 2026-10-05, see Comments)
- [ ] `durationformat` and translator tested through the template seam only; doctests for changed public examples pass
- [ ] **(rewritten 2026-10-05, §11 D8 — decided: delete)** Remove `DateError::InvalidPattern` entirely: delete the variant (`src/date.rs:1314`) and its engine display arm (`src/template/engine/date.rs:747`) — verified 2026-10-05 to be the variant's only two sites in the tree, with zero constructors anywhere and the arm's overflow claim never satisfiable. The translator's unsupported-token path becomes a **template-render error at the engine seam** (the error names the offending token, the dialect, and points at the supported-token docs) — NOT a `DateError`, and not a within-domain error split. Add/adjust tests asserting that render-error shape; no promoted constructor, no reserved-slot comment — the deletion is the decision, not a check-then-choose.
- [ ] `mise run verify` green

## Comments

> *This was generated by AI during triage.*

**2026-10-05 (decisions — maintainer):**

1. **§11 D8 → delete.** `DateError::InvalidPattern` is removed entirely,
   along with its engine display arm. Verified 2026-10-05: the variant has
   zero constructors in the tree (definition + one display arm only) and that
   arm's overflow claim can never be true. The translator's unsupported-token
   path becomes a **template-render error at the engine seam** (the error
   names the offending token, the dialect, and points at the docs) — NOT a
   `DateError`, and not a within-domain error split. This answers the D8
   checklist item and the Triage Notes sub-question below.
2. **Blocking edge amended:** this ticket no longer depends on 05 — no item
   here consumes 05's output; it parallelizes with 05 after 04. (Also recorded
   in the spec's chain note.)

> *This was generated by AI during triage.*

## Triage Notes (2026-10-05)

**What we've established so far:**

- Category `enhancement`, state `needs-info` — items 1–17 are well-specified, but the embedded D8 either/or touches the publicly exported `DateError` enum and the corpus contradicts itself, so `ready-for-agent` did not hold (ticket 04 precedent: open questions get closed by the maintainer before the brief is written).
- Claims verified at current HEAD: `DateError::InvalidPattern` exists with zero constructors and its only other reference is the engine `date_error` match arm; `fn format_with` exists only at the engine site (core copies deleted by 03's remediation); `durationformat` absent (0 hits); no moment-dialect translator anywhere; `%+` never emitted; `DelayedFormat` engine-only.
- Redundancy check: looked in the engine date module, core date/duration format paths, filter registry, tests, research notes — no existing implementation of `durationformat` or dialect translation.
- Prior rejection: no `.out-of-scope/` directory exists — not a previously rejected request.

**What we still need from you (@maintainer):**

- **D8 (gates the ticket)** — promote or delete `DateError::InvalidPattern`? Evidence conflicts: review §3/§4 say unsupported moment tokens become `InvalidPattern`, but the variant's own rustdoc says no path constructs it and the engine already maps chrono failures to minijinja errors. Either branch changes public API. Sub-question: does the translator's unsupported-token error become `DateError::InvalidPattern` (spec D-b) or a template-engine error (variant rustdoc)?
- **Blocker edge** — this ticket is `Blocked by: 04, 05`, but it is template-seam only while 05 is stalled on the query-grammar question (X1). Keep the `05` edge (per spec's 05 → 06 → 07 sequencing) or drop it so 06 can proceed after 04 alone?

## Agent Brief

**Category:** enhancement
**Summary:** Deliver template format parity — a moment-dialect translator and a `durationformat` filter at the template seam — and execute the closed D8 decision: delete the unconstructible `DateError::InvalidPattern` so unsupported tokens surface as a template-render error.

**Current behavior:**
The template engine's private date-format renderer is the only renderer left;
there is no moment-dialect translator and no `durationformat` filter, so
recipes copied from moment-style templates cannot format, and parsed durations
have no humanizing display. The public date error enum still exports an
`InvalidPattern` variant with zero constructors — its sole reference is a
display/format arm whose overflow claim can never be true. The intended error
shape for an unsupported moment token is undecided in code: review material
once pointed at `InvalidPattern`, the variant's own docs said nothing
constructs it.

**Desired behavior:**
- `durationformat` renders human-readable durations ("3 days, 4 hours") using
  magnitude-based, compound unit-list display.
- Common moment tokens render through the template seam: the
  `YYYY MM DD HH mm ss` family, `Do`/`S`, `dddd`/`ddd`/`MMM`/`MMMM`, and
  bracketed literals.
- An unsupported moment token yields a **template-render error at the engine
  seam** whose message names the offending token, identifies the input as
  moment-dialect, and points at where supported tokens are documented — not a
  `DateError`, not a within-domain error split.
- `DateError::InvalidPattern` and its display arm are gone with no call-site
  fallout; nothing else about dialect behavior changes — valid patterns render
  as before and chrono's `DelayedFormat` errors keep their existing mapping.
- Invalid-pattern rendering remains at exactly one site (the engine's private
  format renderer); the translator is a pure function at the template seam —
  no `Dialect` trait — and adds no parallel specifier validator.

**Key interfaces:**
- `DateError`: remove the `InvalidPattern` variant (public enum change) and
  its display arm; no replacement variant is introduced
- `durationformat`: new template filter rendering a humanized duration
- Moment-dialect translator: pure function at the template seam; emits only a
  subset of chrono's documented strftime table, never `%+`
- Unsupported-token failure: a template-render error carrying token +
  dialect + docs pointer, produced at the engine seam

**Acceptance criteria:**
- [ ] `DateError::InvalidPattern` and its display arm are deleted; the crate
      builds with no remaining references and no call-site fallout
- [ ] Feeding an unsupported moment token through the template seam produces a
      template-render error whose message names the token, identifies
      moment-dialect, and points at the supported-token docs — pinned by test
- [ ] `durationformat` renders compound durations (e.g. "3 days, 4 hours"),
      magnitude-based — pinned by test
- [ ] The supported moment token set (`YYYY MM DD HH mm ss`, `Do`/`S`,
      `dddd`/`ddd`/`MMM`/`MMMM`, bracket literals) renders correctly through
      the template seam — pinned by tests
- [ ] `%+` is never emitted (pinned by test); invalid-pattern errors come
      only from chrono's `DelayedFormat`, rendered at exactly one site with no
      parallel specifier validator — regression-pinned so a second copy
      cannot reappear
- [ ] Dialect behavior otherwise unchanged: valid-pattern rendering and chrono
      error mapping are untouched by the deletion; existing format tests pass
      without modification
- [ ] Format rustdoc notes that `%Z` prints only an offset, `%S` may render
      60 (leap second), and week numbers use `%V`/`%G` — never `%U`/`%W`
- [ ] Doctests for changed public examples pass; `mise run verify` green

**Out of scope:**
- The error-taxonomy split (`DateParseError`/`DateError` restructure) —
  explicitly NOT chosen; deleting `InvalidPattern` is not an invitation to
  redesign the error enum
- Date-recognition work (ticket 04) — build on it, don't touch it
- Divergence-register / GLOSSARY.md / ADR records (ticket 08)
- Query temporal functions (ticket 05) — parallelizes with this ticket
- Shorthands / `parse_with` (ticket 07)
