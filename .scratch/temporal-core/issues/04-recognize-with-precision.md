# 04: Recognize with precision — classify entry + precision hoist

**What to build:** Each domain gets exactly one recognition entry point whose contract tells callers "not mine / mine-but-invalid / valid", so no call site can forget a guard; and a recognized string keeps its precision through arithmetic and re-formatting, so a `YYYY-MM` note never silently becomes a day. Demo contract: a `YYYY-MM` value classifies, shifts by a month, and re-formats as year-month — and the template engine's private precision/parsed-date types are gone.

**Blocked by:** 03 (classify/parse paths built on the settled duration shape and calendar owner).

**Status:** ready-for-agent

Skills: `rust-skills`, `rust-unit-testing`, `rust-doc`, `codebase-design`. Rules: `api-parse-dont-validate` (N7, N15, N23), `api-must-use` (classify), `type-enum-states` (`Precision` — not in equality), `pat-exhaustive-enum` (engine `date_shift_unit`'s `match precision` at `src/template/engine/date.rs:480` gains a `YearMonth` arm — no `_`, so a new precision forces an explicit shift decision; recognition matches enumerate every format/unit variant), `err-doc-errors` + `doc-all-public` (`classify`'s `# Errors`, recognition-result/`Precision` docs); D11 doc written per `rust-doc` conventions. Design record: `../review.md` §2.2, §5.3–5.4, §9 (S5+S7).

- [ ] `classify` exists on both domains with the trichotomy contract: `None` = cheap O(1) shape gate (no allocation), `Some(Err)` = allocating detail, `Some(Ok)` = the recognition-result struct the precision hoist lands in — date domain `{ value: DateValue | DateTimeValue, precision: Precision }` (precision must ride the result: `DateValue` wraps only `NaiveDate`, so it cannot distinguish `2026-07` from `2026-07-01`), duration domain `DurationValue` — declared `#[must_use]`, tested through the interface
- [ ] No recognition result discards errors silently: `Some(Err)` is the only detail channel and every parse path it replaces routes through it, with each seam choosing its policy (engine/serde/query strict, lexer/field-coercion lenient fall-back — strictness-by-seam, spec line 116). Note: the engine `ParsedDate::parse` `.ok()` swallow this item originally cited was already closed by ticket 02 (N4 — `src/template/engine/date.rs:235` chains `DateError` as source); the live `Option`-shaped discards are `src/note/field.rs:115` and `src/note/parser/inline.rs:224`
- [ ] Guard functions (`can_start`/`is_iso_shape`/`has_four_digit_year`) are private; the `can_start && parse` call sites and the coercion-cascade protocol copy are retired to `classify` (N7, N15, N23); `TextShape` becomes a caller of `classify`, not a fourth recognition fragment (H3)
- [ ] Format enums own the cascade loop once (`parse_any`-style). Live inventory (review §2.2's "four" is stale — the engine calls `parse_iso` rather than looping): **two** format-cascade loops (`DateValue::parse_iso` `src/date.rs:87`, cascade at `src/date.rs:94`; `DateTimeValue::parse_iso` `src/date.rs:400`, cascade at `src/date.rs:407`) and **three** datetime→date coercion cascades retired to `classify` (`src/field.rs:825`, `src/query/sort.rs:477`, engine `ParsedDate::parse` `src/template/engine/date.rs:210`); the `"1h 1x"` divergence is gone (N8, N20, D1). ISO-prefix scan deduplicated (D6) — live count is **two** inline 4-digit copies (`src/date.rs:115` in `has_four_digit_year`, `src/date.rs:127` in `is_iso_shape`), not ×4
- [ ] `parse`/`parse_prefix` merged as **one scanner, two wrappers**: a single loop parameterized `Whole | Prefix` (spec line 92's "merge the duplicated loops"); `classify`/`parse` take the whole input and carry detail in `Err`, while the prefix wrapper stops at the atom boundary, returns the consumed-byte count, and maps anything invalid to `None` — so `src/note/parser/inline.rs:186` keeps its contract and no caller pays for detail it discards. `"1h 1x"` is one grammar with two shapes: `classify` → `Some(Err(DurationError::UnknownUnit))`, prefix wrapper → `None` (inline falls back to text) — both pinned
- [ ] Note-parser date grammar widened to `YYYY-MM` (spec story 15: inline and YAML-deserialized dates share one grammar): the 10-byte gates `src/note/parser/inline.rs:219` (`advance(pos, 10)`) and `src/note/parser/lexer.rs:247` (`ISO_DATE_LEN`) accept the 7-byte year-month shape through `classify`; prose `2026-07` becomes a date where today it stays text — behavior change, pinned by test (not a divergence-register entry — story 15 forbids the divergence). **Additional gate added 2026-10-05 (§11 D2/X3-G3):** `src/note/field.rs:114` — `.get(..10).and_then(|prefix| DateValue::parse_iso(prefix))` is a third consumption site (prefix-lenient: no atom-boundary rule, so `"2026-01-01junk"` parses, locked by test `src/note/field.rs:833`), production caller `src/note/parser/task.rs:212`, and item 12 lists it only as an `.ok()` discard. It must route through the same widened grammar or story 15 stays half-applied on the task-date String path; its `.get(..10)` also fails on 7-char input after widening. Note: unlike inline/lexer, this site's leniency (no boundary check) is a behavior decision — keep or tighten explicitly, pinned either way
- [ ] `Precision` (`YearMonth` | `Date` | `DateTime`) hoisted to the recognition result; engine's `ParsedDate`/`DatePrecision`/`format_precise` delegate then deleted (D10); precision never participates in value equality (N16); `DatePoint::has_time` (`src/date.rs:929`, struct at `src/date.rs:922`, driving `diff`'s instant/civil split at `src/date.rs:970` — `diff` fn `src/date.rs:965`) derives from the hoisted `Precision` — one precision signal, no third copy
- [ ] **(added 2026-10-05, §11 D6/G5)** `DatePoint::new` takes the hoisted `Precision` (or is replaced by a `from_recognized` constructor) so 03's "a stale precision flag is unrepresentable" becomes true by construction — today `new(wall, instant, has_time: bool)` (`src/date.rs:937-947`) accepts an independent bool, both production sites supply literals (`src/template/engine/date.rs:218` `true`, `:232` `false`), and the invariant holds only by convention. Constructor ownership only — explicitly *not* the rejected `LocalWall` newtype
- [ ] Fractional truncation fixed once behind the hoisted precision (D3), pinned by a round-trip test: a nanosecond-bearing input survives a shift through `date_add` and re-renders its fraction (reference behavior: `to_datetime_string` at `src/date.rs:436`; spec seam-1 line 123 lists the `YYYY-MM` round-trip but not this one)
- [ ] `date_add` unit documentation widened to any `DurationUnit` spelling, with the sub-day-on-date-only caveat, landing with the precision fix (D11; ticket 03 already added the week/calendar-vs-instant clause at `src/template/engine/date.rs:399-417` — the two missing pieces are the "any spelling" wording and the sub-day-on-date-only no-op caveat)
- [ ] `YYYY-MM` precision survives arithmetic round-trip (demo contract above)
- [ ] New/changed rustdoc on `pub` items clean — `# Errors` on `classify`/`parse_any`, recognition-result and `Precision` docs, intra-doc links resolve after the engine deletions: `mise run doc --all-features` clean (`RUSTDOCFLAGS=-D warnings`; `mise run verify` does **not** run cargo-doc — ticket 03 precedent, ticket 09 owns the gate)
- [ ] `mise run verify` green

**Review amendments (2026-10-05 adversarial rust-design pass; source: `../review.md` §11 + component reports §12):**

- [ ] **(repins)** Several line cites above were verified against a tree they no longer match — the pinned positions come from pre-`bcc938b5` work (this ticket cites pinned-in-`07c631ac` without re-verifying; that is how it drifted). All cites in this file are now re-pinned at HEAD `5b7dc748`; follow ticket 03's pattern (its L136–139 cites re-verified at merge): re-verify each cite at implementation time and record the verifying commit rather than trusting these numbers.
- [ ] **(D2) Date-domain prefix entry with consumed length** — items 13 + 16 cannot all hold together today: `classify` returns no consumed length, so item 16's widening leaves the three `.get(..10)`/`advance(pos, 10)`/`ISO_DATE_LEN` gates hardcoding 10 in four places (inline, lexer, `src/note/field.rs:114`, plus the two `parse_iso` scans at `src/date.rs:87`/`:94`), and item 13's "guards private" retires `is_iso_shape` without giving callers a derived boundary. Mirror ticket 03's decision-2 shape (one scanner, two wrappers): either a `DateValue::parse_prefix` returning `(value, consumed)` that every gate consults, or a `classify` variant carrying consumed bytes — then each gate is one call with a derived length, and `"2026-07"` widening propagates automatically. The duration side already has its prefix wrapper (`src/duration.rs` `parse_prefix`, contract pinned at `src/note/parser/inline.rs:186`); the date side does not, and this ticket invents it.
- [ ] **(D2) raw-spelling pin for trailing separators (§11 U6)** — `parse` and `parse_prefix` already disagree on `raw` for `"1h,"`: whole-input stores `trimmed` (`src/duration.rs:214`, so `raw == "1h,"`) while prefix stores the consumed slice (`src/duration.rs:281`, so `raw == "1h"`). The merge in item 15 must pick one and pin it — today only acceptance is pinned (`trailing_comma("1h,")` at `src/duration.rs:1307-1315`), not the stored spelling, so `Display` of the parsed value (`src/duration.rs:775-781` writes `raw`) can silently change under a refactor.
- [ ] **(D7) `DateTimeFormat::parse` return shape** — its signature is `Result<DateTime<Utc>, ParseError>` (`src/date.rs:905-910`) but the `Some(pat)` arm attaches a placeholder (`naive.and_utc()`, `src/date.rs:914`); the caller then recovers the wall clock and resolves locally, undoing the lie (`src/date.rs:421-427`, comment "The shape pass attached UTC as a placeholder"). Documented at `src/date.rs:896-899`, but the type still says UTC. Shape pass should return the naive/Rfc3339 distinction it actually has (an enum or `NaiveDateTime`) so no caller can mistake the placeholder for an instant — small, but it is this ticket's recognition layer.
- [ ] **(X-G) `classify` consumes the calendar flag** — verify the duration recognition result carries `is_calendar` (or the equivalent `DurationUnit` class) so `DurationValue::is_calendar` (`src/duration.rs:461`, an `expect(dead_code)`-gated method per ticket 09) gets its declared consumer through the interface; if not, record the fate explicitly for ticket 09 rather than leaving it to the dead-surface census.
- [ ] **(D1 gate layering) `Some(Err)` must stay behind the `None` shape gate** — the `classify` contract's cheap-first ordering has a live counterexample: `src/note/field.rs:531` runs `DurationValue::parse(trimmed)` (the allocating detail path) on *every* string field value in the `if let Ok` chain with no shape gate. When item 12 routes this site through `classify`, the `None` gate must be consulted first or the coercion path pays allocation cost on all non-duration strings. (Pre-existing behavior — safe, `Err → Self::String` fallback — so performance knowledge, not a bug; spec line 116 is the authority.)
- [ ] **(ordering vs D1) The shift frame lives in the engine** — `date_shift_unit`'s `match precision` dispatches the frame (`src/template/engine/date.rs:469-491`, `shift_wall` call at `:482`) while `shift_wall` is `pub(crate)` at `src/date.rs:1143` solely for that caller, and the "date-only is civil / datetime is instant" policy is stated in five places. If this ticket implements the `YearMonth` arm by *extending the engine match* (item 9's `pat-exhaustive-enum` framing does exactly that), the frame policy gets a sixth home. Prefer landing the narrow fold first (route through `DatePoint`/`DateValue::shift`, engine delegates): see §11 D1 — deliberately left unassigned; **open question: assign it before this ticket or fold it in here?**

## Comments

> *This was generated by AI during triage.*

**2026-10-04 (review — CodeGraph + rust-skills pass, no implementation):**

Seven factual amendments applied in place (checklist above): stale `.ok()`
claim (finding 4), cascade/scan counts (findings 5–6), `DatePoint::has_time`
single-source (7), missing `mise run doc --all-features` gate (8), D3 pinning
test (9), skills-line gaps — `pat-exhaustive-enum`, `err-doc-errors`,
`doc-all-public` (10), and item 11's `Some(Ok)` payload now cited from spec
lines 92–93.

Four open questions (the review's findings 1, 2, 3, 11) needed a maintainer
call **before pickup** — all four decided in the entry below:

1. **`Some(Ok)` payload shape (spec-constrained, confirm).** Spec line 92
   prints `classify(s) -> Option<Result<Value, Error>>` while line 93 hoists
   `Precision` "alongside a recognition-result struct" — the checklist now
   reads `Some(Ok)` = value + `Precision` for dates, value for durations.
   Confirm the struct's name/fields (and whether `DateTimeValue` input keeps
   `Precision::DateTime` even though the value already implies it).
2. **`parse_prefix` disposition + consumed length.** Spec line 92 says
   classify "merges the duplicated `parse`/`parse_prefix` loops", but
   `src/note/parser/inline.rs:186` needs the consumed-byte count, which a
   `classify` returning only `Option<Result<…>>` cannot serve (`"1h 30m"` is
   one atom with no whitespace pre-boundary). Options: `classify` also
   reports consumed length, or inline keeps a dedicated prefix scan (in which
   case item 14's "merged loops" claim needs rewording). Related: pin
   `"1h 1x"` — today `parse` → `Err(DurationError::UnknownUnit)` while
   `parse_prefix` → `None` (`src/duration.rs:203/235`); post-04 both reject,
   but Err-vs-None shape at lenient seams should be stated.
3. **`Some(Err)` seam policy vs item 12's "no parse path discards errors".**
   Strictness-by-seam (spec line 116) means lenient seams *must* keep falling
   back: `src/note/field.rs:115` and `src/note/parser/inline.rs:224` convert
   detail to `None` deliberately, and `src/field.rs:825` /
   `src/query/sort.rs:477` degrade to String/Plain. Wording should be
   "`Some(Err)` carries the detail; each seam picks its policy", not "every
   path surfaces it".
4. **`YYYY-MM` in the note parser (spec story 15 vs code).** Sort, field
   coercion, and serde accept `YYYY-MM`, but the note parser requires 10 bytes
   (`src/note/parser/inline.rs:219` `advance(pos, 10)`,
   `src/note/parser/lexer.rs:247` `ISO_DATE_LEN`), so `"2026-07"` stays text
   inline. Does 04 widen this, or is it a deliberate divergence recorded in
   ticket 08's divergence register?

**2026-10-04 (decisions — maintainer, all four closed):**

1. `Some(Ok)` = recognition-result struct; date domain carries
   `{ value, precision }` (precision must ride the result — `DateValue` wraps
   `NaiveDate` and cannot distinguish `2026-07` from `2026-07-01`); duration
   domain carries `DurationValue`.
2. One scanner, two wrappers: single loop parameterized `Whole | Prefix`;
   `classify`/`parse` carry `Err` detail, the prefix wrapper returns consumed
   bytes and `None` on any invalid input. `inline.rs:186` keeps its contract;
   `"1h 1x"` = `Some(Err(UnknownUnit))` via `classify`, `None` via the prefix
   wrapper — one grammar, two shapes, both pinned.
3. Item 12 wording stands as amended: `Some(Err)` carries the detail, each
   seam picks its policy (strict: engine/serde/query; lenient: lexer, field
   coercion, `TextShape` → fall back).
4. Widen in 04, not a divergence: spec story 15 requires inline and YAML to
   share one grammar incl. `YYYY-MM`. New checklist item added; prose
   `2026-07` becoming a date is an intended behavior change, pinned by test.

> *This was generated by AI during triage.*

## Agent Brief

**Category:** enhancement
**Summary:** Give each domain exactly one recognition entry point (`classify`)
with a not-mine/invalid/valid contract, and hoist precision out of the
template engine into the recognition result so a recognized string keeps its
shape through arithmetic and re-formatting.

**Current behavior:**
Recognition is fragmented. The shape guards (`can_start`,
`is_iso_shape`, `has_four_digit_year`) are crate-visible protocol functions
that every caller must know to combine with `parse`; the query sort
classifier, the YAML field-value classifier, and the engine's private
date/time parser each hand-roll their own datetime-then-date fallback; the
duration `parse` and `parse_prefix` entry points duplicate their outer
scanning loops (with a `"1h 1x"` divergence between them); and each date
format enum carries its own copy of the try-each-pattern-until-one-works
cascade loop. Precision exists only inside the template engine, as a private
precision enum plus a parsed-date wrapper plus a re-serialization helper —
outside the engine a recognized value has no recorded shape, so `YYYY-MM`
input can come back as a day, fractional seconds are truncated on
re-serialization, and `DatePoint::has_time` acts as a second, independent
precision signal. The note parser additionally demands a full 10-byte date,
so `YYYY-MM` stays plain text there even though YAML and query seams accept
it.

**Desired behavior:**
- Each domain (date, duration) exposes one `classify` entry point returning
  `Option<Result<…>>`, declared `#[must_use]`: `None` = cheap O(1)
  shape gate with no allocation (the lexer hot path must not regress),
  `Some(Err)` = allocating detail, `Some(Ok)` = the recognition result. On
  the date domain the result is a struct carrying `{ value, precision }` —
  precision must ride the result because the date value alone cannot tell
  `2026-07` from `2026-07-01`. On the duration domain it is the duration
  value itself.
- Every recognition site becomes a `classify` caller: the guard functions go
  private, the `can_start && parse` idiom and the datetime-then-date coercion
  cascades disappear from sort/text-shape, YAML field coercion, and the
  engine; `TextShape` is a caller, not a fourth recognition fragment.
- Strictness stays per-seam: `Some(Err)` is the detail channel, engine/serde/
  query seams surface it, lenient seams (note lexer, field coercion, text
  shape) deliberately fall back to String/Plain/`None`.
- The duration scanner exists once, as one loop parameterized whole-input vs
  prefix: the whole-input forms carry `Err` detail; the prefix form stops at
  the atom boundary, reports consumed bytes, and maps any invalid input to
  `None` (the inline note parser keeps its contract). `"1h 1x"` is one
  grammar with two pinned shapes: `Some(Err(UnknownUnit))` through
  `classify`, `None` through the prefix form.
- Format enums gain one cascade-owning `parse_any`-style constructor; adding
  a recognized shape later means one variant + one list entry + one pattern.
- `Precision` (`YearMonth` | `Date` | `DateTime`) lives at the recognition
  layer; the engine's private precision/parsed-date/re-serialization trio
  delegates to it and is then deleted; `DatePoint::has_time` derives from it
  (one precision signal); precision never participates in value equality.
- Fractional seconds survive arithmetic re-serialization (fixed once behind
  the hoisted precision), and the note-parser date grammar accepts `YYYY-MM`
  so inline and YAML dates share one grammar (spec story 15) — prose
  `2026-07` becomes a date.
- `date_add`/`date_sub` rustdoc widens to "any `DurationUnit` spelling" with
  the sub-day-on-date-only no-op caveat.

**Key interfaces:**
- `classify` (new, both domains): `Option<Result<…>>` + `#[must_use]`; the
  only recognition entry point
- Recognition result struct (date domain): `{ value, precision }`
- `Precision`: hoisted enum; excluded from equality; feeds `DatePoint`'s
  time-of-day flag
- Format enums: `parse_any`-style constructor owning the cascade loop
- Duration scanning: one loop, two wrappers (whole-input vs prefix-with-
  consumed-length); guards become private
- Deleted by end of ticket: the engine's private precision enum, parsed-date
  wrapper, and precise-format helper; no second precision signal remains
- Visibility: no new crate-public surface is required — recognition stays
  crate-internal (exports were settled by ticket 01)

**Acceptance criteria:**
- [ ] Demo contract: a `YYYY-MM` input classifies with year-month precision,
      shifts by one month through the calendar owner, and re-formats as
      `YYYY-MM` — never widened to a day — pinned by test
- [ ] Trichotomy pinned behaviorally through the interface: non-temporal text
      → `None`; shape-matching-but-invalid input → `Some(Err)` whose detail
      names the failure; valid input → `Some(Ok)` with the right precision;
      `classify` is `#[must_use]`
- [ ] `"1h 1x"` pinned both ways: `Some(Err(UnknownUnit))` via `classify`,
      `None` via the prefix wrapper
- [ ] A note body containing `2026-07` parses as an inline date (behavior
      change from plain text), pinned by test
- [ ] No production call site hand-rolls datetime-then-date fallback or
      combines a shape guard with `parse`; the guards are unreachable outside
      their own module
- [ ] Two values recognized at different precisions but denoting the same
      day compare equal (precision absent from equality) while rendering at
      their own precision
- [ ] A nanosecond-bearing input shifted through `date_add` re-renders its
      fractional seconds, pinned by round-trip test
- [ ] The template engine contains no private precision/parsed-date/
      re-serialization types, and its existing date-filter tests still pass
- [ ] `date_add` rustdoc names any `DurationUnit` spelling and the sub-day-
      date-only caveat
- [ ] `mise run verify` green and `mise run doc --all-features` clean
      (`RUSTDOCFLAGS=-D warnings` — verify does not run cargo-doc)
- [ ] Every checklist item in this ticket's body is checked

**Out of scope:**
- CONTEXT.md clauses, ADRs, and divergence-register entries (ticket 08) — and
  no divergence entry is recorded for `YYYY-MM` inline parsing, since
  story 15 requires the shared grammar
- Dead-surface deletions and the doc-gate task ownership (ticket 09)
- Query temporal functions (05), template format dialects (06),
  shorthands/`parse_with` (07)
- Duration semantics and calendar-owner behavior settled by ticket 03
  (`parts`, bit-exact Σ, week-as-calendar, month clamping) — build on them,
  don't re-litigate
- DST resolver / `TzGuard` machinery from ticket 02
- Flipping lenient seams strict: field coercion must keep degrading
  non-date-shaped strings to String, text shape to Plain
- New crate-public exports

