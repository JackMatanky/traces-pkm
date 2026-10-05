# 05: Query temporal functions

**What to build:** The query language speaks Dataview's headline idiom: `date + duration`, `date - duration`, `date - date` yields a duration, and `date_add` / `date_diff` / `date_component` answer period-bucketing questions — all as registry entries, no new operators or grammar. Demo contract: a filter expression that adds `dur("1 month")` to a note's date and compares the result against another date returns the same answer as the template engine would.

**Blocked by:** 03 **and** 04 (needs the calendar owner — 03 — behind a stable recognition/classification interface — 04; encoded chain 03 → 04 → 05; the original header named only 04 and buried 03 as a parenthetical, which review found to be the wrong shape because the dependency is a transitive *seam*, not just a schedule).

**Status:** ready-for-agent — decisions X1(i)/X7(a)/D1 closed 2026-10-05 (see Comments); brief attached
**Category:** enhancement — new query capability closing a Dataview parity gap; nothing existing is broken.

Skills: `rust-integration-testing`, `rust-skills`. Rules: `FilterFunction` registry = open/closed (no grammar changes), `conv-tryfrom-fallible` for `date_component` extraction; edge strategies named per workflow. Design record: `../review.md` §3 (B17/B18/B24), §9 (S3); spec stories 1–5.

- [ ] `date + duration` / `date - duration` evaluate in filter expressions (B17)
- [ ] `date - date` yields a duration (B17)
- [ ] Duration arithmetic (`+`, `-`, `* number`) available to queries (B18)
- [ ] `date_add` / `date_diff` / `date_component` work, with `date_add` accepting any `DurationUnit` spelling; `date_component` covers year/month/day/… per B24 (Dataview's date components minus the `.week` footgun) — bucketing helpers (start/end-of-week/month/year: `sow`/`eow`/`soy`/`eoy` plus month variants, B11) are ticket 07's deliverable per spec story 4, not claimed here — date components read chrono accessors directly (`year()`, `iso_week()`, …); the week `date_component` is ISO via `iso_week()` (never `%U`/`%W` semantics)
- [ ] Every public workflow and edge failure (null operand, wrong type, out-of-range `date_component`) maps to an integration case or an explicit out-of-scope reason, tested at the `FilterFunction` registry seam
- [ ] Engine and query paths agree on the same calendar owner (no reimplementation in the query layer **or the template engine**) — the engine currently performs calendar arithmetic directly: `succ_opt()`/`pred_opt()` (`src/template/engine/date.rs:125`/`:135`), `with_day(1)` (`:575`), `with_day(num_days_in_month)` (`:587`), `weekday()` (`:602`), `leap_year()` (`:691-692`), plus the `shift_date(closure)` seam (`:376-397`) beside `date_shift_unit`. `src/date.rs:26-33` claims calendar semantics live in exactly one place — currently false (§13 T-new-1). These route through the owner or get named exceptions here. Ticket 07's bucketing/sow/eow helpers must not add new direct-chrono sites (07 is walled off from this AC, but this AC is its upstream constraint)
- [ ] `mise run verify` green

**Review amendments (2026-10-05 adversarial rust-design pass; source: `../review.md` §11):**

- [ ] **(X1 decided 2026-10-05 — option (i); implement the enumerated surface.)** Spec L77's grammar lock has been amended to name a minimal value-expression surface, and stories 1–3 plus this ticket's demo contract stand verbatim. Implement exactly those six forms — field references; literals; registry function calls returning values (arguments are themselves value expressions, which is the two-field-arg convention); `+`/`-` on date/duration values (including `date − date` → duration); `*` by a number; comparison operators whose operands may be any value expression (including function results) — and nothing else. No operator precedence beyond left-to-right, no boolean algebra beyond the existing filter combinators, no user-defined functions. The amendment is recorded at spec L77 and is the authority; do not restate or re-lock it here. The current grammar must grow from its literal-only argument parser (`FilterFunction` one-variant predicate enum, `src/query/grammar/filter.rs:111-120`) to carry function results as values so `dur("1 month")` in a filter parses, `date − date` can feed a comparison, and `date_add(...)` results are legal comparison operands — that is the grammar work the decision unblocks.
- [ ] **(X7 decided 2026-10-05 — option (a).)** Wire compound `date_add`: a multi-part duration reaches `DateValue::apply`/`DateTimeValue::apply`, applied in **written** order, and the behavior is pinned through the query seam (the demo/bucketing path is the production consumer spec decision 12 declared — the `expect(dead_code)` status of `apply` and the six-site census end here). This ticket therefore does NOT leave `apply` unconsumed: ticket 09's "wire or delete" test becomes verify-wired, with no keep-with-rationale exemption. Same bucket, same consumer path: `DurationValue::parts` and `from_seconds` are exercised by the compound route rather than exempted. *(Related: `date_add` still accepts any single `DurationUnit` spelling — item 4 stands; this item is about the multi-part case.)*
- [ ] **(added 2026-10-05, §13 P5 / §11 D4) Unify the two f64→`TimeDelta` conversion algorithms on one correct owner.** Today two algorithms exist with different failure modes: `seconds_delta` (`src/date.rs:1119-1129`, errors `DateError::OutOfRange`) and `TryFrom<DurationSeconds> for TimeDelta` (`src/duration.rs:1043-1062`, errors `DurationError::NonFiniteSeconds` — the variant name lies for finite out-of-range input). They DIVERGE (the earlier corpus equivalence claim at `../review.md §12.1 C4` was corrected 2026-10-05): `0.9999999996` → `TryFrom` returns `Err(NonFiniteSeconds)` while `seconds_delta` yields `+1s`; `-1e-10` → `Err` vs `0` (because `TimeDelta::new(…, 1e9)` returns `None` while `TimeDelta::nanoseconds` carries). Consolidate on one correct `TryFrom` (`DurationSeconds` is the lawful home — orphan rules forbid `TryFrom<f64> for TimeDelta`); absorb the carry logic; keep the `DateError::OutOfRange` mapping at the date sites but stop erasing `.source()` (`.map_err(|_| DateError::OutOfRange)` at `src/date.rs:212` and `:603` violates the house source-chain rule). Pins: both divergence inputs through BOTH entries (seam-1, spec L125 now lists them). Error-vocab naming (`NonFiniteSeconds` mislabel) is gated on the spec's dual error-naming decision — record the gate, don't invent names. Not live today (`parts == None` reaches only via `from_seconds`, zero prod callers, plus test-only mixed arithmetic); both algorithms go live together when this ticket wires `apply`.
- [ ] **(D1 sequencing resolved 2026-10-05.)** Route new query-side arithmetic through the calendar owner's public entry landed by ticket 04 — the `DatePoint`/`DateValue::shift` shape — never through the engine's `date_shift_unit`. The engine becomes a *caller* of that entry, not the owner. Ticket 04 (blocking this one, and before this ticket starts) carries the shift-frame/precision fold including its `YearMonth` arm, so by the time items 1–3 expose arithmetic to queries the owner entry exists; no query-side call site may add a sixth home for the frame policy (`engine/date.rs:469-491` policy copy).
- [ ] **(added 2026-10-05, §13 ND-3) State and pin the true fractional-remainder rule — two docs contradict the code.** `src/duration.rs:956-963` scopes the nominal ratios to "identity and ordering **only**", yet `src/date.rs:253` and `:648` **apply** them (`fract × fixed_seconds`); sibling docs `src/date.rs:617-619` and `:643-645` claim the remainder "is still calendar time" when it is `fract × 30-day nominal`. Consequences: `0.5mo` = 15 nominal days, and `whole = 0` skips the month shift entirely. Fix both doc sites to state the real rule (sub-day units apply exactly on the instant; fractional day/week/month/year remainders multiply by fixed nominal ratios during application), add pins (seam-1), and hand the Temporal-divergence angle (Temporal rejects fractional months) to ticket 08's register.
- [ ] **(added 2026-10-05, §13 NU-1 — note only, decision lives in 08)** A computed duration's `Display` is not a faithful encoding of its parts: `parse("1mo") + parse("0s")` keeps parts `[(1, Month), (0, s)]` but re-synthesizes `raw` to `"4w 2d"` via `canonical_raw` (which omits Month/Year), and `parse("1mo") * 2.0` prints `"8w 4d"` while applying 2 calendar months. Spec L84/L90 already decide the mechanism (raw is display-only; the witness is never encoded in raw), so this ticket records the observation only — it does NOT re-open the mechanism. The register entry and any Display/Serialize pin for a computed value are ticket 08's / seam-1's (spec L125 now lists the pin).

## Comments

> *This was generated by AI during triage.*

**2026-10-05 (decisions — maintainer):**

1. **X1 → option (i).** The spec's grammar lock has been **amended** (spec L77)
   to name a minimal value-expression surface; stories 1–3 and this ticket's
   demo contract stand verbatim. The surface, exhaustively: field references;
   literals; registry function calls returning values (their arguments are
   themselves value expressions — this covers the two-field-arg convention);
   `+`/`-` on date/duration values (including `date − date` → duration);
   `*` by a number; and comparison operators whose operands may be any value
   expression. No operator precedence beyond left-to-right, no boolean
   algebra beyond the existing filter combinators, no user-defined
   functions. Everything outside that fence returns to the lock. This closes
   the X1 open-decision item and the Triage Notes' expression-surface
   question: implement the surface, don't restate the stories as
   function-only predicates and don't touch the demo contract.
2. **X7 → option (a).** This ticket wires the compound path: `date_add` with
   a multi-part duration routes through `DateValue::apply` /
   `DateTimeValue::apply` in written order, and the behavior is pinned
   through the query seam. That is the production consumer spec decision 12
   declared, so `apply` stops being `expect(dead_code)` here. Ticket 09 gets
   **no** exemption — its wire-or-delete test becomes verify-wired — and
   `DurationValue::parts` / `from_seconds` ride the same consumer path.
   Closes the X7 item and notes.
3. **D1 sequencing resolved.** The shift-frame/precision fold lands in
   ticket 04 (before this ticket starts, and including its `YearMonth` arm),
   so the owner entry (`DatePoint`/`DateValue::shift`) exists before this
   ticket's arithmetic lands. New query-side call sites route through that
   owner entry; the engine remains a caller, not the owner. Any item or note
   treating the engine frame as this ticket's to-decide is resolved.

> *This was generated by AI during triage.*

## Triage Notes (2026-10-05)

**What we've established so far:**

- Category `enhancement`, state `needs-info` — the body's own X1 block says the ticket is undeliverable as written until the grammar question is ruled on, so `ready-for-agent` did not hold.
- Claims spot-verified at current HEAD: `FilterFunction` is a one-variant predicate enum (`Contains`) with literal-only args; `dur(`/`date_add`/`date_diff`/`date_component` have zero hits under `src/query/`; the six-site dead-code census (X7) matches `src/date.rs` / `src/duration.rs`; `date_shift_unit` frame policy at the cited engine location. Value-level `Add`/`Sub`/`Mul` already exist on `DurationValue` (B18's type half is done).
- Redundancy check: no query-side value-expression/arithmetic implementation exists (looked in the filter grammar, registry, and `src/query/` broadly).
- Prior rejection: no `.out-of-scope/` directory exists — not a previously rejected request.
- Cites re-pinned in this pass: spec grammar lock L75 → L77; `filter.rs` span 110-121 → 111-120. (Cross-file drift resolved 2026-10-05: review §11 X1's "L75" was corrected to L77 in the same pass.)
- Blockers: 01–03 resolved; 04 is `ready-for-agent` with an agent brief — this ticket's own decisions are now the front edge.

**What we still need from you (@maintainer):**

- **X1 (gates the ticket)** — spec's grammar lock vs stories 1–3: (i) amend the spec lock to name a minimal expression surface (function-as-value + two-field args + an op on function results), or (ii) restate stories 1–3 as function-only predicates and amend the demo contract at the top of this ticket. Both change spec + demo contract — must not be picked silently during implementation.
- **X7** — multi-part `apply` consumer: (a) `date_add` with a compound duration lands in this ticket, giving `DateValue::apply`/`DateTimeValue::apply` a consumer, or (b) ticket 09 gets an explicit keep-with-rationale exemption (same bucket: `DurationValue::parts`, `from_seconds`). Ticket 09 cannot finish its core AC until this is answered.
- **D1 sequencing (light)** — when the query arithmetic lands, must it route through the calendar owner's public entry (shift-frame fold already assigned elsewhere), or is D1 still unassigned? Ticket 04 carries the parallel "before 04 or in 04?" question — settle both together to avoid double-handling.

## Agent Brief

**Category:** enhancement
**Summary:** Teach the query language Dataview's temporal idiom — `date ± duration`, `date − date` → duration, and `date_add`/`date_diff`/`date_component` as registry entries — on top of a minimal, spec-locked value-expression surface.

**Current behavior:**
The query grammar parses only predicate-shaped filters: `FilterFunction` is a
one-variant predicate enum whose argument parser accepts literals only, so
there is no value-expression node at all. `dur("1 month")` inside a filter is
a syntax error, nothing can carry `date − date`'s duration result, and a
`date_add(...)` result cannot be a comparison operand. The template engine
already answers all of these questions; the query layer cannot ask them. The
type-level arithmetic (`Add`/`Sub`/`Mul` on `DurationValue`) exists — the
missing half is grammar and evaluation.

**Desired behavior:**
- Filter expressions may build and compare *values*, not just test them,
  using exactly the surface the amended spec lock enumerates: field
  references; literals; registry function calls returning values (arguments
  are themselves value expressions); `+`/`-` on date/duration values
  (including `date − date` → duration); `*` by a number; and comparison
  operators whose operands may be any value expression.
- Evaluation is left-to-right with no operator precedence; boolean
  combination stays exactly where it already lives (the existing filter
  combinators).
- `date_add` / `date_diff` / `date_component` are `FilterFunction` registry
  entries that work in filters, with `date_add` accepting any
  `DurationUnit` spelling. `date_component` covers year/month/day and the
  remaining components via chrono accessors; its week answer is ISO
  (`iso_week()`), never `%U`/`%W` semantics.
- A multi-part `date_add` duration routes through the date/duration `apply`
  path in written order, so compound durations behave the same from a filter
  as from a template.
- All query-side date arithmetic reaches the calendar owner's public entry
  (the shift/diff surface ticket 04 lands); the template engine is a caller
  of that entry, not a second owner, and the query layer adds no copy of the
  civil-vs-instant frame policy.
- The demo contract holds: a filter expression that adds `dur("1 month")` to
  a note's date and compares the result against another date returns the same
  answer the template engine gives.
- Edge behavior is explicit: a null operand, a wrong-typed operand, and an
  out-of-range `date_component` each produce a defined result or a defined
  error — never a panic and never silent coercion.

**Key interfaces:**
- Query value-expression node (new): parses the six enumerated forms and
  evaluates to a value; anything outside the fence fails to parse
- `FilterFunction` registry: entries may now return values usable as
  operands, in addition to predicate entries; entry signatures stay the seam
  tests bind to
- `date_add` / `date_diff` / `date_component`: registry entries with the
  existing engine semantics as the reference behavior
- Date/duration `apply(base, &DurationValue)` path: gains its production
  consumer through compound `date_add`; `DurationValue::parts` and
  `from_seconds` are exercised along that path
- Calendar-owner shift/diff entry (from ticket 04): the only route for
  calendar meaning on the query side
- Arithmetic operators on `DateValue` / `DateTimeValue` / `DurationValue`
  (already present at the type level): consumed by the expression evaluator

**Acceptance criteria:**
- [ ] A filter expression adding `dur("1 month")` to a note's date and
      comparing the result to another date evaluates and returns the same
      answer as the equivalent template expression (demo contract), pinned
      by an integration test at the registry seam
- [ ] `date + duration` and `date − duration` evaluate inside filter
      expressions, and `date − date` yields a duration that can itself feed
      a further arithmetic operation or comparison — each pinned by test
- [ ] Duration `+`, `-`, and `* number` are usable in query expressions,
      with left-to-right evaluation pinned (no precedence): an expression
      where a precedence-aware evaluator would differ evaluates
      left-to-right
- [ ] `date_add`, `date_diff`, and `date_component` work as query registry
      entries; `date_add` accepts every `DurationUnit` spelling and a
      compound (multi-part) duration applies through the written-order
      `apply` path, pinned by test through the query seam
- [ ] `date_component` covers year/month/day and the remaining components
      via chrono accessors, is ISO for week, and an out-of-range component
      returns a defined error — pinned by test
- [ ] Null operands and wrong-typed operands produce defined, pinned
      behavior at the registry seam (no panics); every public workflow and
      edge failure is either an integration case or carries an explicit
      out-of-scope reason
- [ ] The expression grammar accepts exactly the enumerated six forms: a
      construct from outside the fence (a boolean operator inside a value
      expression, a precedence grouping, a user-defined function) fails to
      parse, pinned by test
- [ ] No query-side or engine-side second copy of the calendar frame policy
      exists; query arithmetic and the template engine both reach the
      calendar owner's public entry (ticket 04's), pinned by inspection in
      review
- [ ] Stories 1–3 of the spec pass end-to-end under the enumerated surface
      and the spec grammar lock is not amended again by this ticket
- [ ] `mise run verify` green
- [ ] Every checklist item in this ticket's body is checked

**Out of scope:**
- Anything outside the enumerated fence: operator precedence, boolean
  algebra beyond the existing filter combinators, user-defined functions,
  and any further grammar growth — all remain locked
- Any further spec re-lock or amendment of the grammar-lock bullet (the
  2026-10-05 amendment is the authority; do not restate it)
- Template-side work: format dialects (ticket 06) and shorthands /
  `parse_with` (ticket 07) own those surfaces
- Bucketing helpers (`sow`/`eow`/`soy`/`eoy` and month variants) — ticket
  07's deliverable per spec story 4
- Recognition/classification and precision-hoist work (ticket 04), duration
  semantics settled by ticket 03, dead-surface deletion and the doc gate
  (ticket 09), divergence register / CONTEXT.md / ADR entries (ticket 08)
- New crate-public exports not required to satisfy the acceptance criteria
