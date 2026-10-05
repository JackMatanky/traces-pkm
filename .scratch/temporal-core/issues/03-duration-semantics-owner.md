# 03: Duration semantics end-to-end — parts, single registry, calendar owner

**What to build:** `dur("1 month")` means one thing, declared in one place: the value's identity is its magnitude (fixed ratios, named `fixed_seconds`), its parsed shape is retained, and applying it to a date is calendar-correct through a single calendar owner that the template engine delegates to. Demo contract: `dur("1 month") == dur("30 days")` as values yet they shift a date differently — demonstrated by a pinning test, documented in the glossary.

**Blocked by:** 01, 02. *(Header amended 2026-10-05: previously read `None (01 and 02 merged)` — a resolved ticket's provenance should record its actual blocking edges, and both dependencies were true blockers that merged: 01 in `c990f6c9`, 02 merged before this ticket's work began. Matches spec L143's ordering — the seam chain runs after correctness — and ticket 01's own text, "ticket 03 (`Blocked by: 01`) can proceed once this lands".)*

**Status:** resolved — implemented on `duration-semantics-owner` (`57ea1b09`; remediation: `fe4c01e2` rs-review-spec, `2e4304d2` rs-review-design, `bcc938b5` rs-review-arch + rust-design audit)

Skills: `rust-skills`, `rust-unit-testing`, `rust-doc`, `codebase-design`. Rules: `api-parse-dont-validate` (Σ invariant by construction), `type-enum-states` (parts as regime witness), `pat-exhaustive-enum` (regime-witness and calendar/fixed unit matches enumerate every `DurationUnit` variant — no `_`/`u =>` arm, so adding a unit forces a calendar-vs-fixed decision, backing item 29), `api-operator-overload` (`Sub` = negated-`Add` and non-finite-yielding `Mul` justified in rustdoc), `num-overflow-explicit`, `num-float-compare` (bit-exact Σ fold, `Mul` non-finite), `err-result-over-panic` (owner out-of-range → `Result`; `Mul` never panics), `err-doc-errors` + `doc-all-public` (owner fns' `# Errors`, narrowed `DurationValue` type docs), `anti-over-abstraction` (no `Clock`/`TimeZone` traits — one adapter = hypothetical seam); DST pinning tests use ticket 02's `TzGuard` (`src/tz_guard.rs`) — `TZ` injected per test, never shared/global (spec line 122); calendar application uses chrono's own primitives (`checked_add_months(Months)` / `checked_add_days(Days)` on the local naive value) — no hand-rolled month arithmetic; zone conversions mirror ticket 02's rule verbatim: local→UTC via `and_utc()`/`naive_utc()`; UTC→local wall clock for calendar application (spec D12) via offset-based checked arithmetic (`Local.offset_from_utc_datetime(&naive_utc)` then `naive_utc.checked_add_offset(offset.fix())`, `None` → out-of-range error); never `.naive_local()` (documented `# Panics` at range edge — chrono `expect("Local time out of range…")`) and never `.naive_utc()` where the local wall clock is wanted (wrong frame). Design record: `../review.md` §4 (A2′), §5.1–5.2, §9 (S1+S2).

- [x] `DurationValue` gains `parts: Option<Box<[(f64, DurationUnit)]>>` alongside seconds; entries carry the **written** magnitude with the whole-duration sign applied (N15b example corrected: `"-1h 30m"` → `[(-1.0, Hour), (-30.0, Minute)]` — spec line 82 prints `(-0.5, Minute)`, which Σs to −3630 against a stored −5400; the spec line 82 typo is corrected alongside this ticket). Exactly two filling *behaviors*: (a) **one shared text fold** behind both text entries — `parse` **and** `parse_prefix` (live today at `src/duration.rs:148`, called from `src/note/parser/inline.rs:155`, retired by ticket 04's `classify`) — so identical text cannot carry a different regime before 04; and (b) duration arithmetic (pinned below). No state where `seconds` and a `Some` witness disagree; no runtime validation on either path
- [x] Σ invariant pinned by a **bit-exact** test: folding the stored `parts` in left-to-right order reconstructs stored `seconds` with `==` (no tolerance; signed entries) — for parsed values **and** `Some`+`Some` arithmetic results. Arithmetic therefore computes `seconds` as **that same fold over the combined entries** (`Add`: concat; `Sub`: negated rhs concat; `Mul`: scaled entries), *not* as `lhs.seconds ± rhs.seconds` — float non-associativity makes those differ (`dur("10000000000000000s") + dur("1s 1s")`: direct = `10000000000000002`, fold = `10000000000000000`), which is why "seconds add" below is pinned as value-level behavior only. Mixed `Some`+`None` ⇒ `None` (direct seconds arithmetic — no witness, no Σ claim). Compare `DurationSeconds` values (its `PartialEq` is `total_cmp`-based, construction normalizes `±0.0`) to stay clear of `clippy::float_cmp`; raw `f64` `==` needs `#[expect(clippy::float_cmp, reason = …)]`. Spec lines 82, 87, 123 amended accordingly
- [x] `Add`/`Sub` semantics pinned by tests: for `Some`+`Some`, `seconds` is the single L→R fold over the combined entries (Σ survives bit-exactly by construction; `Sub` = concat with each rhs entry negated); for a `None` operand, `seconds` adds directly (`Sub` = add of negated rhs) and the result is `None`; `-0.0` normalized; equality stays seconds-only (fold-based and direct results can differ by ≤1 ulp — never pin cross-path arithmetic equality on rounding-edge fixtures); value-level pin: `dur("1h") + dur("30m") == dur("1h 30m")`; result `raw` re-synthesized through `from_seconds` (spec line 87)
- [x] `Mul<f64>` scales the `parts` entries (`Some` ⇒ `seconds` = fold of the scaled entries; `None` ⇒ `seconds` scales directly), pinned by a test; a non-finite scalar or overflow leaves a non-finite `seconds` that **every consuming conversion rejects** (`NonFiniteSeconds`), documented by a narrowed "always finite (at construction)" type-doc clause, and **never panics** — pinned by a test; and the `raw` a non-finite value re-synthesizes through `from_seconds` is pinned too (today's fallback yields `"infs"` — `src/duration.rs:269-271`; either accept that spelling or route non-finite to `DurationSeconds`'s dialect without the `s` suffix, but pin whatever ships — never a fabricated `"0s"`)
- [x] `from_seconds` stores `None` (honest: cannot synthesize Day/Week/Month/Year shape); regime witness matches exhaustively: day/week/month/year parts ⇒ calendar, else fixed, `None` ⇒ fixed
- [x] `seconds()` renamed `fixed_seconds()` (and `seconds_i64` → `fixed_seconds_i64`) at every call site; calendar meaning exposed nowhere except behind the calendar owner
- [x] Exactly one unit-ratio table: `fixed_seconds()` remains the sole ratio definition; `from_seconds`'s greedy ms-table is rebuilt from a `const` list of the six fixed sub-year units with ratios derived from `fixed_seconds()` (no literal ratio in the function; `phf::UNIT_MAP` stays names-only, so it is not a second ratio copy); **`fixed_seconds_i64()` derives from the same definition too** — today it is a second literal table (`src/duration.rs:637-648`, incl. Month/Year), its `Millisecond → None` stays a variant match rather than a ratio, and the derivation's casts carry `#[expect(clippy::as_conversions, reason = …)]` (const float→int derivation compiles — verified); Month/Year omission from magnitude conversion documented (N18, N2); rename scope confirmed: only `DurationUnit`'s ratio methods rename — `DurationValue::to_seconds()` (`src/duration.rs:296`) is the identity accessor and stays
- [x] Calendar owner exists in the date module: `shift(base, n, unit)`, `diff(a, b, unit)`, `apply(base, &DurationValue)` applying parts left-to-right in written order; the engine's copied shift/diff deleted and delegation in place (H4 shrinks). The owner takes and returns **typed values** (`DateValue` / `DateTimeValue` / `NaiveDateTime` + unit) — never strings, never formatting: the engine keeps `ParsedDate`/`format_precise` until ticket 04, so this ticket changes *where arithmetic lives*, not how results are re-serialized.
- [x] A2′ pinning test demonstrates equal values shifting dates differently (`dur("1 month")` vs `dur("30 days")`); the cross-spelling equality contract (`dur("1h 30m") == dur("90m")`) stays pinned green; CONTEXT.md clause (c) text is ticket 08's job — this ticket supplies the executable test it cites
- [x] `checked_add`/`checked_sub` superseded by `apply` (removed or re-backed — no API pretending to arithmetic it can't do); ticket 09 verifies no residue remains
- [x] `shift`/`apply` preserve the local wall clock (spec D12): `DateTime<Utc>` → local naive → chrono calendar add → back through the resolver; month clamping matches chrono's documented behavior (Jan 31 + 1 month → last day of Feb), pinned by a test citing chrono's own example. `DateValue` shift/diff is pure civil arithmetic (zone-free inputs never touch the resolver); `diff` preserves the engine's declared split — instants when both sides carry a time component, civil wall clocks when either side is date-only.
- [x] `day` is a calendar application unit (spec D13): `1d == 24h` as values but they shift differently across a DST transition — the extended A2′ incoherence pinned by test; sub-day units apply as exact durations. `week` is a calendar application unit ≡ 7 wall-days (spec D13 amendment): `1w` and `7d` shift identically, `1w` and `168h` differ across DST — both pinned; the engine's previous fixed-second week behavior is deliberately replaced (research items 24/25). Regime change touches `shift`/`apply` only: `date_diff` stays **measurement** for every unit (instant/civil split unchanged, `src/template/engine/date.rs:762-802`) — do not "fix" diff along with shift; no existing test pins the engine's old fixed-week *shift* behavior (verified — only unit-parse tests mention week), so the flip starts green
- [x] `apply`'s left-to-right order pinned by a test: a two-part duration with one calendar and one sub-day part (`"1d 1h"`) applied across a DST transition yields the hand-computed L→R result and differs from the reversed order (spec line 84)
- [x] `signed_years_since`/`signed_months_since` relocated from the template engine into the calendar owner (chrono has no months-between primitive — this custom code is justified and single-owned here)
- [x] New/changed rustdoc on `pub` items clean — `# Errors` on every new `Result` path, `DurationValue`'s "always finite" clause narrowed to construction paths, intra-doc links resolve: `mise run doc --all-features` clean (`RUSTDOCFLAGS=-D warnings`; `mise run verify` does **not** run cargo-doc — see ticket 09)
- [x] `mise run verify` green

## Comments

> *This was generated by AI during triage.*

**2026-09-29 (review — rust-skills + CodeGraph pass, no implementation):**
four amendments before pickup:

1. Σ/`Add` inconsistency resolved (Option A): `Some`+`Some` results compute
   `seconds` as the same L→R fold over combined entries — "seconds add" and
   bit-exact Σ over arithmetic results were jointly unsatisfiable under
   float non-associativity (counterexample in item 12).
2. N15b example corrected to `[(-1.0, Hour), (-30.0, Minute)]`; written-
   magnitude convention stated; spec line 82 typo fixed alongside.
3. `parse_prefix` named as the second live text path (until 04's
   `classify`); both entries share one fold (spec line 82 amended).
4. `fixed_seconds_i64()` derivation added (was a second literal table);
   skills line gains `rust-doc`, `pat-exhaustive-enum`,
   `api-operator-overload`, `err-doc-errors`, `TzGuard`, and E8 adds the
   `mise run doc --all-features` gate.

**2026-10-02 (resolution):**

- Implemented in `57ea1b09`, review remediation in `fe4c01e2` (branch
  `duration-semantics-owner`). Full gate green: `mise run verify` (3109
  unit + 71 doc tests, strict clippy) and
  `RUSTDOCFLAGS=-D warnings cargo doc --all-features` clean.
- Review remediation: fractional calendar-unit remainders (`1.5d`) now
  round-trip through the local wall clock like their whole-unit prefix
  (D12); the bit-exact Σ invariant is pinned with raw `f64` `==` under
  `clippy::float_cmp`; `Sub`'s negated-rhs witness, mixed `Some`+`None`
  `Sub`, and `Mul` overflow are pinned; `shift`-path DST behavior
  (`1w` ≡ `7d`, `1w` vs `168h`, `shift` ≡ `apply`) pinned in
  `mod calendar_owner`; engine `date_shift_unit` doc names weeks as
  calendar units.
- Item 22's Luxon verification: Luxon treats weeks as higher-order
  variable-length calendar units ("adjusts for them when working with
  higher-order, variable-length units like days, weeks, months, and
  years", luxon `docs/zones.md`) — matches the implemented `1w ≡ 7d`
  wall-day semantics, so no divergence-register entry is needed.

**2026-10-02 (design-review remediation):**

- `date_diff_measurement`'s fixed-unit arm enumerated (was a `u =>`
  catch-all, violating `pat-exhaustive-enum`); adding a unit now forces
  the calendar/fixed decision there too.
- `DateValue::shift` delegates to `shift_wall` on midnight (per-unit
  semantics in exactly one place); `shift_date_months` deleted, along
  with its `dead_code` expectation.
- Duration grammar accepts exponent floats (`"1e3s"`, `".5e-3"`): the
  `from_seconds` fallback renders the spec-pinned `DurationSeconds`
  dialect (exponent outside `[1e-6, 1e15)`), so synthesized spellings
  now round-trip through `parse` (pinned for `1e300`, `-1e300`,
  `1e-300`); fold-overflow to `NonFiniteSeconds` pinned (two finite
  `1e308` parts).
- Least privilege: `fold_parts` and `DurationParts` made private;
  `TryFrom<&DurationValue> for TimeDelta` deleted (zero production
  callers; owned impl retained).
- D13 weeks amendment completed in the engine: `date_add`/`date_diff`
  docs and `unknown_unit_error`'s message now name weeks.
- Docs unified: `DateError` and both module docs name arithmetic
  failures and the calendar owner; `shift_wall` contract stated.
- Tests: `tomorrow/today/yesterday` render in one template (removes the
  documented midnight-rollover flake); duplicate
  `extracts_wrapped_naive_date_via_into_inner` deleted; engine
  `date_diff` gains a `weeks` case. Full gate green: 3117 unit + 71
  doc tests, strict clippy, `cargo doc -D warnings` clean.

**2026-10-02 (rs-review-arch + rust-design remediation, committed as `bcc938b5`):**

- Calendar-owner seam deepened: `DatePoint` now carries `has_time` and
  owns the measurement (`DatePoint::diff`); the free
  `date_diff_measurement` and its caller-computed `both_datetimes`
  flag are gone, and the engine's `ParsedDate` holds a `DatePoint`
  directly, so a stale precision flag is unrepresentable.
- Engine owns an explicit `DateError` → render-error translation:
  `LocalZoneLookup` (broken tz-data) keeps its distinct diagnosis and
  source instead of being mislabeled as arithmetic overflow.
- Deleted the seven test-only dead methods (`DateTimeValue::
  {start_of_day, to_offset_string, to_time_string, to_date_string,
  cmp_date, format_with}`, `DateValue::format_with`) and their tests:
  seven `expect(dead_code)` suppressions eliminated.
- Duration parser: per-part scan extracted (`scan_part`), the sign
  rule got a single owner (`digits_after_sign`); `parse_number`
  CC 17→14, NPath 648→360, `parse_prefix` NPath 240→under threshold.
- `Add`/`Sub` merged into one `combine` implementation; synthesis
  spelling extracted (`canonical_raw`, single-`String` accumulation);
  `from_seconds` is now a composition over it.
- The whole/sub-second split in `apply_part` has one owner
  (`seconds_delta`, four call sites → one).
- `UNIT_HINT` lives beside the unit registry and feeds the engine's
  unknown-unit message (ends the drift that hid weeks); docs and
  message now name `ms`, which the registry always accepted.
- Least privilege: `shift_months`, `signed_years_since`,
  `signed_months_since` are private; `shift_wall`'s and
  `date_shift_unit`'s error docs no longer claim a "no whole-second
  value" failure that cannot occur.
- Tests: months/years local-wall shifts across DST (was 0% covered),
  civil fractional calendar parts, negative civil shifts, truncated-
  exponent rejections, engine `ms` pins, engine error-translation
  pins; two format-table tautologies and the duplicated From-promotion
  test deleted; four identical filter-render rstest bodies share one
  helper. Gate green: 3113 unit + 71 doc tests, strict clippy,
  `cargo doc -D warnings` clean; CRAP over-threshold functions in the
  cluster fell from six to two (coverage on flagged paths
  56-69% → 78-94%).

**2026-10-03 (final shape & handoff notes):**

Checkpoint after the full review/audit cycle (`57ea1b09` → `bcc938b5`,
+548/−645 net over the audit round). Where each responsibility now
lives — checklist line numbers above predate the remediations and are
stale; these symbols are the current pointers:

- `src/duration.rs` — `DurationValue::parse`/`parse_prefix` share the
  per-part scan `scan_part`; the sign rule has one owner
  (`digits_after_sign`, used by `parse_number` and
  `can_start_duration_segment`); `Add`/`Sub` delegate to `combine`;
  synthesized spellings come from `canonical_raw` (single-`String`
  greedy decomposition, `DurationSeconds` Display dialect fallback);
  `fold_parts`/`DurationParts` are private; `UNIT_HINT` sits beside
  `UNIT_MAP` as the machine-owned "expected units" hint the engine
  formats into its unknown-unit error.
- `src/date.rs` — `DatePoint { wall, instant, has_time }` is the
  measurement seam (`DatePoint::diff`; the free
  `date_diff_measurement` is gone); `shift_wall` is the civil
  per-unit core, `shift_months`/`signed_years_since`/
  `signed_months_since` are module-private; `seconds_delta` owns the
  whole/sub-second split for both `apply_part` impls; `local_zone`
  resolution (`local_naive_to_utc`, `resolve_gap_offset`) unchanged.
- `src/template/engine/date.rs` — `ParsedDate` holds a `DatePoint`;
  arithmetic filters delegate to the owner and translate `DateError`
  through one `date_error` fn (`LocalZoneLookup` keeps its diagnosis
  and typed source; everything else maps to the range error).

Deliberate remaining state (do not "fix" without reading the audit):

- `DurationValue::parts`, `is_calendar`, `from_seconds`,
  `DateValue::shift`/`apply`, `DateTimeValue::apply` still carry
  narrowly-scoped `expect(dead_code)` — they are the ticket's
  spec-mandated surface with tests as first consumers; the query
  temporal functions (ticket 04's `classify`, index-query#05) are the
  declared production consumers.
- `DurationValue` reports LCOM4=2 and complexity >50 under `mess`
  (advisory): the parse-machinery vs value/arithmetic method clusters
  of one cohesive domain type. Disposition: no split — the module is
  the spec-mandated single registry; splitting would fragment the
  owner without relocating knowledge.
- The calendar-vs-fixed unit classification is intentionally repeated
  as exhaustive arm shape in `is_calendar`, `shift_wall`,
  `DateTimeValue::shift`, both `apply_part`s, and `DatePoint::diff`:
  `pat-exhaustive-enum` makes the compiler the policy owner (adding a
  unit forces the decision everywhere). Do not replace the matches
  with a shared `is_calendar()` predicate — that would bypass the
  exhaustiveness gate.
- Residual CRAP over-threshold: `DateTimeValue::shift` (17.4 @ 78.1%
  cov) and `shift_wall` (15.1 @ 93.8%) — inherent per-unit match
  branchiness, both improved from 21.9 @ 68.8%.
- Deferred (named, not lost): optional Criterion bench for
  `DurationValue::parse`/`from_seconds` if the `canonical_raw`
  allocation shape is ever questioned; no duration/date bench exists
  in `benches/`, and the audit's performance notes on these paths are
  labeled hypotheses.
