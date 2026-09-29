# Comprehensive Review v4 — `src/date.rs` + `src/duration.rs`

Companion to `spec.md` (the tracker feature `temporal-core`): the full findings/analysis/deepening record. Tickets cite IDs from here rather than re-deriving them.

**Status:** decisions A2′, B1+local-naive, D-b(scoped), and D11-doc-widening are **accepted**. This document is the full findings/analysis/deepening record. No implementation performed.

**Post-v4 evolutions (settled at spec time, marked inline below):** DST policy = ambiguous → `.earliest()`, nonexistent → shift forward (was the single open item); null ordering = keep current behavior, ADR replaces the code comment; the three test seams confirmed (value types / `FilterFunction` registry / template filters); calendar frame = local wall clock for day/week/month/year application (chrono `checked_add_*` + local round-trip; Temporal/Luxon/PG parity), sub-hour units exact, incoherence extended to `1d` vs `24h` across DST (pinned by test); week joins day as a calendar application unit (≡ 7 wall-days; moment/Temporal parity — research items 24–25); one gap-verified local→UTC resolver (`MappedLocalTime` exhaustive match; tz-data errors must not shift; rustdoc cited to Temporal `'compatible'`/RFC 5545 and jiff `Compatible`; wasm caveat chrono #1701 out-of-scope); format grammar formally bound to `chrono::format::strftime` (intra-doc link, no parallel invalid-pattern validator, `%+` forbidden, interop `…Z` via `to_rfc3339_opts(Secs, use_z=true)`), week bucketing mandated to `iso_week()`/`from_isoywd_opt`; external prior-art research completed (see `research/`), backing the divergence register.

---

## 1. Seam landscape

| Seam                | Module                                  | Adapters today                                                        | Missing / broken                                                     |
| ------------------- | --------------------------------------- | --------------------------------------------------------------------- | -------------------------------------------------------------------- |
| Recognition/parsing | `date.rs`, `duration.rs`                    | note lexer/inline/list, `TextShape`, field coercion, engine `ParsedDate`  | derived `Deserialize` bypasses it (N12); guard protocol leaks (N15/N7) |
| Formatting          | `date.rs` (`format_with`, `Display`)          | engine `format_precise` + engine's own `format_with`                      | moment-dialect input (D-b → S8); `durationformat` (B14 → S4)           |
| Calendar arithmetic | **no owner**                                | engine `date_shift_unit`/`date_diff` (copy), dead `checked_add`/`checked_sub` | query-side arithmetic, `weekday`, shorthands — all blocked on S1       |
| Unit registry       | `duration.rs` `UNIT_MAP`/`DurationUnit`       | duration parse, engine kwarg + shift                                  | regime declaration (N2 → S2); **4th silent ratio copy** (N18)            |
| Clock               | **no doctrine**                             | `Local::now` ×4, `Utc::now` ×3 (engine)                                   | naive-zone = local (decided), display policy (D9 → S10)              |
| Precision           | engine-private `DatePrecision`/`ParsedDate` | arithmetic re-serialization only                                      | recognition result should own it (N16/D10 → S7)                      |

Deletion test: every value type earns its keep (grammar + registry + error taxonomy reappear across ≥6 call sites). Row 3 is the structural root of N2/D1/D9; row 6 is a seam an adapter had to invent because the public one lost information.

---

## 2. Findings catalogue

### 2.1 Correctness

| ID   | Finding                                                                                                     | Evidence                        | Rule                    |
| ---- | ----------------------------------------------------------------------------------------------------------- | ------------------------------- | ----------------------- |
| **N1**   | `chrono::Duration::seconds` panics on out-of-range magnitude → `try_seconds`                                    | engine `date.rs:432`              | `err-result-over-panic`   |
| **N3′**  | `parse("-0m")` → `-0.0`, `total_cmp` orders strictly below `0.0` so zero can fail `>= 0`; plus `0.1+0.2` equality class | `duration.rs:111–121, 717–747`    | `num-float-compare`       |
| **N4**   | No `.with_source(DateError)` chaining; `ParsedDate::parse` discards via `.ok()`                                   | engine `date.rs:183–201`          | `err-source-chain`        |
| **N7**   | Missing `can_start` guard in coercion cascade → non-durations hit allocating path/misclassify                 | `note/field.rs:525`               | `api-parse-dont-validate` |
| **N14**  | `from_seconds(1e300)` → `"0s"`; same class at `0.0004` → `"0s"` (ms-round drops value) — Display lies about `seconds` | `duration.rs:224–269`             | `type-display-vs-debug`   |
| **N15b** | `"-1h 30m"` whole-duration sign rule documented but unpinned by tests                                         | `duration.rs:45–47`               | —                       |
| **N19**  | `DurationSeconds: Display` = raw `f64` — `1e300` renders 301 digits                                               | `duration.rs:737–741`             | `type-numeric-fmt`        |
| **D9**   | Clock mix, no doctrine                                                                                      | engine `date.rs:92–117, 699, 709` | —                       |
| **D3**   | Fractional truncation in `format_precise`                                                                     | engine `date.rs:282`              | —                       |

### 2.2 Interface coherence

| ID  | Finding                                                                                                                                                                           | Remedy                                                                           |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| **N2**  | Dual unit semantics: fixed 30d/365d in `seconds()`/`TimeDelta` vs calendar in engine `match` — undeclared                                                                               | A2′ + S1 + S2                                                                    |
| **N8**  | `parse`/`parse_prefix` hand-copied loops; `"1h 1x"` divergence                                                                                                                          | S5                                                                               |
| **N12** | Derived `Deserialize` bypasses `parse_iso`: chrono `NaiveDate::from_str` skips 4-digit guard + rejects `YYYY-MM`; datetime RFC3339-only; serial `…Z` ≠ Display; `NoteFieldValue` carries both | S6 (manual impl, `duration.rs:561` precedent)                                      |
| **N15** | Guard-protocol leak (`can_start`/`is_iso_shape`/`has_four_digit_year` are protocol functions); `field.rs` forgot the protocol precisely because the interface demands hidden knowledge    | S5 — classify, guards go private                                                 |
| **N16** | `YYYY-MM` → day 1, shape forgotten; precision belongs to the recognition result                                                                                                     | S7 (hoist `Precision`)                                                             |
| **N17** | `DateValue` public, `DateTimeValue`/`DateError` `pub(crate)`; external `FromStr::Err` unnameable; no `#[non_exhaustive]` on either error                                                      | S6                                                                               |
| **N18** | `from_seconds` carries a private 4th copy of unit ratios (ms-table, `duration.rs:236–243`) omitting Month/Year undeclared                                                             | S2 (derive from registry; document omission)                                     |
| **N20** | Cascade loop copied between `DateValue::parse_iso` and `DateTimeValue::parse_iso`                                                                                                     | S5-era: `parse_any` on format enums                                                |
| **N21** | Invalid-pattern detection exists 3× (`date.rs:255`, `date.rs:463`, engine `date.rs:255`)                                                                                                | S4: shared `render_pattern`                                                        |
| **N23** | `From<FieldValueRef>` embeds classification policy (null→link→duration→string) inside a `From` impl                                                                                   | S5: duration step → `classify`                                                     |
| **D11** | Doc ≠ behavior: `date_add` docs promise six units; `DurationUnit::parse` accepts `weeks`/`ms`/abbrevs                                                                                     | **decided: widen doc, bundle with S7** (the real trap is precision no-ops, S7 fixes) |

### 2.3 Structural / depth debt

| ID  | Finding                                                                                                                                                                                                                                      |
| --- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **D1**  | Format cascade duplicated ×4 (date/datetime × parse/engine)                                                                                                                                                                                  |
| **D6**  | ISO-prefix byte scan duplicated ×4                                                                                                                                                                                                           |
| **D7**  | `NaiveDate`/`DateTime<Utc>` leak via `into_inner`/`From` — **demoted**: `NaiveDate` is already validated, so this is surface-area, not a parse-don't-validate hole. Keep the `From` impls; shrink the *need* (S1/S7); re-run deletion test on `into_inner` after |
| **D10** | Engine-private `DatePrecision` + `ParsedDate` + `format_precise` = a recognition-result type an adapter had to build because the public seam lost precision — direct evidence for N16                                                              |
| **H3**  | Recognition is not one seam: `TextShape` + three guard fragments of one classifier                                                                                                                                                             |
| **H4**  | Engine is a fat adapter: owns calendar arithmetic, formatting, clock                                                                                                                                                                         |
| **—**   | Dead bridge: `checked_add`/`checked_sub` unusable until A2′ makes them calendar-aware (or superseded by S1's `apply`)                                                                                                                              |
| **—**   | 8 methods behind `#[cfg_attr(not(test), expect(dead_code))]` — Decision-C: wire (S4 wires `format_with`) or delete (`to_offset_string` may become interop serial, `to_time_string`/`start_of_day`/`cmp_date` likely delete)                              |

---

## 3. Parity assessment (condensed)

### Dataview (22,124 lines, L-cited)
**At parity:** ISO recognition incl. `YYYY-MM`; multi-unit durations (superset with `y`/`ms` — keep); cross-spelling value equality (stronger than theirs, E4); template-side arithmetic; `file.ctime/cday/mtime/mdate`; task date fields; midnight coercion; null-placement superset.

**Gaps:** **B17** query `date ± duration`, `date − date` (their headline idiom L13501/L4807) → `FilterFunction` entries (`filter.rs:116–125`), never operators; **B18** duration arithmetic (follows); **B14** `durationformat` (spec L6803–6825); **B12** parse-with-format (L6075); **B11** `sow/eow/soy/eoy` (L14960); **B24** components minus `.week`; **B26/B30** `file.day` (L3652).

**Divergences to register:** UTC instants (tz-bug pedigree #1412/#601 — *strengthened by the local-naive decision: parse is now TZ-dependent by design, storage stays UTC*); null never satisfies an ordering (**ADR needed — currently a comment**; *decided at spec time: keep current behavior, ADR replaces the comment*); explicit `NoteFieldType` rank; canonical duration Eq; space-datetime acceptance (note `TextShape` reclassification side effect); strftime-dialect.

**Do-not-port:** `.week`=`floor(day/7)+1`; epoch-falsy; Luxon `YYYY` week-year; null-min comparisons; `today()`/`now()` parse-then-fail; alphabetical type ordering.

### Templater (13,045 lines, L-cited)
**At parity:** day offsets; `tomorrow`/`yesterday`; arbitrary format output; `start/end_of_month`.

**Gaps:** **T1** ISO-8601 `P1M`/`P-1M` offsets (L4973) → adapter translation at `date.now`, not a second grammar; **T2** `reference`/`reference_format` (L4968) → converges with B12, one `parse_with`; **T3** `weekday(n)` (L4992), pinned ISO Monday; **T4** moment tokens (L10978/11106) → D-b; **T5** 100% local clock (L4963) → B1.

**Do-not-port:** type-overloaded `offset`; `P-1M` internal sign; lenient reference guessing; locale weekday; `stat.ctime` lie; unvalidated format strings (we keep `InvalidPattern`).

---

## 4. Decisions (final)

### A2′ — accepted
- `mo/y` stay parseable; `DurationValue` retains `parts: Option<Box<[(f64, DurationUnit)]>>` alongside `seconds`.
- **Identity stays seconds-based** (`fixed_seconds()` ratios, renamed to carry the regime in the name); **application** to dates is calendar for Month/Year parts via one owner (S1), applied left-to-right in written order.
- The Luxon-style incoherence (`"1mo" == "30d"` but different date-shifts) is **accepted and declared**, not hidden — every reference implementation runs this dual regime; ours stops pretending otherwise. **Amendment:** prose alone won't tame it — add a pinning test that *demonstrates* the behavior (equal values, different shifts) so the contract is executable, plus the CONTEXT.md clause.
- A1′ (reject `mo/y`) breaks Dataview L4814 and Templater L4973; A3′ (shape identity) breaks pinned `"1h 30m" == "90m"`. Rejected.

### B1 + naive-input = **local → UTC on parse** — accepted (revised)
- Instants stored as `DateTime<Utc>`; **naive input (`2026-07-29 14:30`) is interpreted in the local zone and converted to UTC at parse** (parity with both plugins; avoids wrong-by-hours comparisons against `file.mtime` for non-UTC users — the prior "keep UTC" lean optimised for test convenience, not user correctness).
- Date-only values are civil dates — no zone applies; `DateValue` untouched.
- `now`/`today`/`tomorrow`/`weekday`/file-stat **formatting** from the local clock; storage always UTC.
- **DST ambiguity policy for local parse — SETTLED at spec time (was this review's single open item):** ambiguous fall-back → chrono `.earliest()`; nonexistent spring-forward → shifted forward by the gap; documented in the divergence register, pinned by a test. (Original options were `.single()` reject / `.earliest()` / `.latest()`.)
- Tests inject `TZ` to stay deterministic (the engineering cost, accepted).

### D-b — accepted, scoped
- moment→strftime translator **at the template seam only**; `DateValue::format_with` keeps exactly one grammar (strftime).
- **Scope 1:** bracket literals `[Daily]`, `YYYY MM DD HH mm ss` family (pass-through), `Do`/`S` ordinals, `dddd`/`ddd`/`MMM`/`MMMM`. Everything else → `InvalidPattern`.
- **Error message is part of the design:** must name the offending token, identify the input as moment-dialect, and point at the supported list — a bare "invalid pattern" on `Do MMM` would be baffling.
- Strictness is deliberate: silently mistranslating an ambiguous token (`YYYY` week-year) would be *worse* than erroring. Full token coverage grows additively later (OCP).

### D11 — widen doc, bundle with S7
- `date_add` docs state "`unit` accepts any `DurationUnit` spelling" — never a second, smaller registry (that's N18 again).
- The real user trap isn't the vocabulary — it's **precision truncation**: sub-day units on date-only inputs format back through `%Y-%m-%d` and visibly do nothing. That's D3/N16, fixed by S7; the doc line and S7 land together.

### Also fixed (carried)
- Start-of-week = ISO Monday, documented divergence.
- `DEFAULT_DATE_FORMAT` removal from `config/model.rs:49` → substitute **`crate::date::DEFAULT_DATETIME_FORMAT`** (not `DEFAULT_DATE_FORMAT` — different value).

---

## 5. Type & pattern designs (Rust-specific)

### 5.1 `DurationParts` — A2′ without dual authority
```text
DurationValue { raw: Box<str>, seconds: DurationSeconds, parts: Option<Box<[(f64, DurationUnit)]>> }
```
- **Sole identity = `seconds`** (Eq/Ord/Hash unchanged). `parts` is the parsed *shape*, `None` for synthesized values (`from_seconds` can't know Month/Year — and its ms-round means exact-parts claims would be false anyway; N14 class stays honest as `None`).
- **Invariant by construction:** `parse` computes `seconds` by a single left-to-right fold over exactly the parts it stores — no state where they disagree; duration arithmetic preserves Σ by construction too (below). `api-parse-dont-validate` satisfied without runtime checks.
- **`parts` doubles as the regime witness:** `Some` containing `Day|Week|Month|Year` ⇒ calendar application; else fixed. `None` ⇒ always fixed (correct). No extra enum; the state is data, matched exhaustively.
- **Signed entries:** parse stores each entry with the whole-duration sign applied (N15b), so Σ is one left-to-right fold.
- **Exactly two filling paths:** `parse` and duration arithmetic. `Add` concatenates when both `Some`; `Sub` concatenates with each rhs entry negated (Σ survives); `Mul` scales entries; **either side `None` ⇒ `None`** — Σ forces it, and the regime then honestly reads `None` ⇒ fixed.
- Rejected: deriving `seconds` on demand (dies on `None`, O(k) Eq for nothing); always-`Some` with approximations (violates Σ invariant = representable invalid state).

### 5.2 Declared regimes
- `DurationUnit::seconds()` → **`fixed_seconds()`**, `seconds_i64()` → `fixed_seconds_i64()` — regime in the name.
- Calendar meaning gets **no** counterpart method on the unit — it exists only behind S1's `shift`, so exactly one place can define it (locality).

### 5.3 `classify` — both domains, one protocol each
```text
DateValue::classify(s)     -> Option<Result<DateValue,     DateError>>      // #[must_use]
DurationValue::classify(s) -> Option<Result<DurationValue, DurationError>>
```
- Trichotomy *is* the guard protocol: `None` = cheap shape gate (O(1), no allocation — preserves lexer hot path), `Some(Err)` = allocating detail, `Some(Ok)`.
- Replaces `can_start && parse` at `sort.rs:473`, fixes N7 at `field.rs:525`, kills N8's loop divergence, absorbs N23's protocol copy; `has_four_digit_year`/`is_iso_shape`/`can_start` go **private** (N15 closed, interface shrinks).
- `TextShape` becomes a *caller*, not a fourth fragment.
- Rejected: one combined date-or-duration mega-classifier — the date/duration/link/string *decision* is coercion-layer policy (cohesion: policy at coercion seam, recognition at value seams).

### 5.4 Precision hoist (N16/D10)
```text
pub(crate) enum Precision { YearMonth, Date, DateTime }
// parse_iso unchanged (clean value for Eq); precision-carrying entry for re-serialization
```
- Engine's `ParsedDate`/`DatePrecision`/`format_precise` delegate → shadow interface deleted; `YYYY-MM` precision survives end-to-end; D3 fixable once.
- `Precision` never enters `DateValue`'s `Eq` — recognition-layer fact, not value identity (`type-enum-states`, not two bools).

### 5.5 Serde repair (N12/N17)
- Manual `Deserialize` via `parse_iso` (`Cow<'de, str>` + `Error::custom`) — exact `duration.rs:561–569` house precedent over `#[serde(try_from)]` (consistency wins).
- One serial spelling per channel: serialize via `Display` (round-trips through `parse_iso` by construction); RFC3339 `…Z` reserved for explicit interop (`to_offset_string`).
- `#[non_exhaustive]` on `DateError` + `DurationError` — within-crate exhaustive matches unaffected.

### 5.6 Smaller mechanisms
- **N18:** delete ms-table; derive greedy units from one registry `const`; document Month/Year omission (un-synthesizable from magnitude).
- **N21:** private `render_pattern(display, pattern) -> Result<String, ()>` in `date.rs`; three call sites map its error to their own type.
- **N20/D1:** `DateFormat::parse_any` / `DateTimeFormat::parse_any` own the cascade loop → adding a shape = variant + `ALL` + `pattern()`, one file.
- **B18:** `Add`/`Sub` for `DurationValue` (seconds add; `Sub` = add of negated rhs; parts concat when both `Some`, rhs entries negated on `Sub`; either side `None` ⇒ `None` — Σ forces it; `-0.0` normalized; result `raw` re-synthesized via `from_seconds`); `Mul<f64>` scales both — non-finite scalar/overflow leaves `seconds` non-finite, declared in the type docs and rejected by consuming conversions (`NonFiniteSeconds`), never panics; Eq remains seconds-only.
- **N19:** choose a rendering dialect for `DurationSeconds` Display (short/general, exponent above threshold) — user-facing once `durationformat` surfaces it.

### 5.7 Considered and rejected (`anti-over-abstraction`)
`Clock`/`TimeZone` traits (one adapter = hypothetical seam — doctrine is data + docs); `Dialect` trait (D-b is a pure function at the template seam); typestate parse builders (interface growth, no state to protect); merged `DateError`+`DurationError` (distinct failure domains); `Cow<'a, str>` for `raw` (lifetime param on an index-stored value = interface bloat); locale first-day-of-week (pinned ISO Monday).

---

## 6. Principles audit (summary verdicts)

| Principle                | Verdict                                                                                         |
| ------------------------ | ------------------------------------------------------------------------------------------------ |
| Separation of concerns   | ❌ calendar arithmetic ownerless (S1); precision split across seam (D10/S7); Display mixing into raw `f64` rendering (N19) |
| Single responsibility    | ❌ `From<FieldValueRef>` classifies + converts (N23/S5); `DurationUnit`'s third job (calendar vocabulary) becomes legitimate only under A2′ + S1 |
| Cohesion & coupling      | ❌ engine reaches *through* registry and newtypes into chrono; target graph `note/query/template → date.rs\|duration.rs → chrono`; `date/` directory split only if S1+S7+S11 push size — don't preempt |
| DRY                      | ❌ ×4 cascades (D1/N20), ×4 scans (D6), ×4 ratio tables (N18), ×3 format-with (N21), ×2 grammars (N8), ×4 guard sites (N15) — all have single-point remedies |
| Modularity               | ❌ guard protocol changes rippled to callers (N15) — modularity's "change inside, not outside" failed; S5 restores it |
| Information hiding       | ❌ protocol functions `pub(crate)` → go private under S5; `into_inner` demoted to monitored surface (D7); hidden facts promoted to normative CONTEXT.md (§7) |
| Open/Closed              | ✅ `FilterFunction`, filter tables, format enums, `UNIT_MAP` — **parity features are almost entirely registry entries**; only A2′/B1/D-b + S1/S2/S5/S7 touch core. ❌ engine unit-match (per-unit arms + shortcuts) |
| General flexibility      | S1's `shift`/`diff`/`apply` serve N callers from one implementation; rigidity of `checked_add`'s fixed regime resolved by A2′; flexibility as *data* (pinned Monday, declared regimes), never single-adapter traits |
| Law of Demeter           | ❌ `can_start && parse` (protocol fiddling), chrono round-trips in `ParsedDate` — one call each after S5/S1 |
| Least astonishment       | N3′ `-0.0`, N14/D11 no-op shifts, N12 Display≠serde, strictness-by-seam invisible → all documented/pinned; **the local-naive flip was itself a least-astonishment correction** |

---

## 7. CONTEXT.md revisions

- **(a)** Funnel claim made normative **and true**: "every date-shaped string — including deserialization — funnels through `parse_iso`" (true only after S6).
- **(b)** Owner of anchored arithmetic named: "`date.rs` owns calendar shift/diff; template/query layers are thin adapters."
- **(c)** Duration regime clause per A2′: identity = magnitude (ratios declared); application = calendar for month/year parts, left-to-right; equal values may shift dates differently (matches Luxon) — backed by a pinning test.
- **(d)** Date interface facts: four-digit-year rule; `YYYY-MM` → day 1; precision at the recognition layer (S7); **naive datetime = local zone → stored UTC** (S10).
- **(e)** Editorial: entries use their own *Avoid* terms inside definitions — stop, or define *Avoid* once as "never as a concept name."
- **(f)** "Single owner of unit registry" true without caveat after `fixed_seconds()` + S1.
- **(g)** Strictness-by-seam rule (lenient coercion / strict template / strict serde) + pinned ISO Monday + DST ambiguity policy.

---

## 8. Rust-skills conformance

| Rule                                    | Status                                                    |
| --------------------------------------- | --------------------------------------------------------- |
| `api-parse-dont-validate`                 | ❌ N12, N15/N7 → S6/S5; ✅ §5.1 invariant-by-construction |
| `serde-try-from-validate`                 | ✅ DurationValue precedent; ❌ date.rs → S6               |
| `err-custom-type`/`err-source-chain`        | ✅ enums; ❌ N4                                           |
| `num-float-compare`/`num-overflow-explicit` | ✅ `total_cmp`/`try_from`; ⚠ N3′; ❌ N1                       |
| `type-enum-states`                        | ✅ proposed (`Precision`, parts-as-witness); ❌ today (D10) |
| `type-display-vs-debug`/`type-numeric-fmt`  | ❌ N14, N19                                               |
| `api-non-exhaustive`                      | ❌ N17                                                    |
| `conv-tryfrom-fallible`                   | ✅ `TryFrom<DurationValue> for TimeDelta`                   |
| `anti-over-abstraction`                   | ✅ §5.7 rejection list                                    |
| `proj-pub-crate-internal`                 | ⚠ N17                                                     |
| `num-float-compare` (S1/S2) | ➕ bit-exact Σ fold + `Mul` non-finite, pinned by ticket 03 |

---

## 9. Deepening plan (final, ranked)

| #   | Suggestion                                                                                                                                                                                                            | Seam / interface                     | Collapses                                       | Parity        |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------ | ----------------------------------------------- | ------------- |
| **S1**  | Calendar-application owner in `date.rs`: `shift(base, n, unit)`, `diff(a, b, unit)`, `apply(base, &DurationValue)` (parts, L→R)                                                                                               | interface on `DateValue`/`DateTimeValue` | engine copy, dead bridge, H4, future query impl | C10, L13506   |
| **S2**  | `parts: Option<…>` + `fixed_seconds()` rename + N18 registry merge                                                                                                                                                        | additive; single constructor         | N2 declared as data, N18                        | A2′           |
| **S3**  | Query temporal functions `date_add`/`date_diff`/`date_component` + duration arithmetic                                                                                                                                      | `FilterFunction` entries only          | —                                               | **B17, B18, B24** |
| **S4**  | `durationformat` + wire `format_with` + shared `render_pattern`                                                                                                                                                             | template filters                     | N21, dead-code attrs                            | B14           |
| **S5**  | `classify` for both domains; guards private; `parse_any` on format enums                                                                                                                                                  | one `#[must_use]` entry per type       | N8, N7-class, N15, N23, N20, D1                 | —             |
| **S6**  | Serde repair: manual `Deserialize`, Display serial spelling, `#[non_exhaustive]`                                                                                                                                          | `duration.rs:561` precedent            | N12, N17                                        | —             |
| **S7**  | Hoist `Precision`/`Recognized*`; engine delegates; D11 doc widened **lands here**                                                                                                                                             | recognition result owns precision    | D10, N16, D3, D11                               | —             |
| **S8**  | D-b translator (scope 1: brackets + common tokens + good errors) + `durationformat`                                                                                                                                     | pure functions, template seam        | T4                                              | T4            |
| **S9**  | Shorthands `sow/eow/soy/eoy`, `weekday`, ISO-8601 offset translation                                                                                                                                                      | engine adapters over S1              | —                                               | B11, T1, T3   |
| **S10** | Correctness pass: N1, N4, N3′, N14, N19, D9 + **local-naive parse + DST policy**, N15b pinning, A2′ incoherence pinning test                                                                                              | —                                    | —                                               | T5            |
| **S11** | `parse_with(text, fmt)` with `Result` contract                                                                                                                                                                            | serves `reference` + future LSP        | —                                               | B12, T2       |
| **S12** | `file.day` at index time                                                                                                                                                                                                | index computes, query exposes        | —                                               | B26, B30      |
| **S13** | Docs: CONTEXT.md (§7), ADR null-ordering, divergence register (space-datetime, magnitude-vs-calendar, strftime-dialect, **local-naive + DST**, strictness-by-seam), config-const removal (`DEFAULT_DATETIME_FORMAT` caveat) | —                                    | —                                               | C-audit       |

---

## 10. Execution order

1. ~~Decisions~~ — **done**: A2′, B1+local-naive, D-b(scoped), D11-widen, ISO Monday.
2. **Correctness:** S10 (N1, N4, N3′, N14, N19, D9, local-naive + DST policy, pinning tests) + S6 (N12/N17).
3. **Load-bearing seam:** S2 → S1 → S5 → S7 (parts → calendar owner → classifier → precision) — this sequence unblocks everything structural.
4. **Parity features:** S3 (biggest gap) → S4, S8, S9, S11, S12.
5. **Docs:** S13.
6. **Dead surface:** Decision-C deletions/wirings (`to_time_string`, `start_of_day`, `cmp_date`, `to_offset_string`→serial?, `checked_add/sub`→superseded by `apply`, `from_seconds`→keep or re-target).

## External prior art (post-v4)

- `research/sql-temporal-conventions.md` — SQL engines' temporal conventions (PG/MySQL/SQLite/DuckDB/Trino/BigQuery); backs: UTC-storage precedent, DST gap-forward + PG-later-vs-earlier overlap divergence, magnitude interval equality, ISO Monday in PG `date_trunc`, null-ordering disagreement among engines.
- `research/general-temporal-libraries.md` — Temporal/Luxon/Java/Python/standards; backs: DST `compatible`/earlier consensus, dual civil-vs-zoned timelines, magnitude duration equality, strict-parse norm, CLDR locale-data weeks.
- `research/rust-temporal-ecosystem.md` — chrono/time/jiff/Arrow/DataFusion; backs: `MappedLocalTime` semantics + gap/error conflation, chrono `Days`/`Months` vs `TimeDelta` regime split, Arrow Duration-vs-Interval, chrono strftime as the de-facto Rust dialect, known chrono critiques (RUSTSEC, serde history).

**Post-v4 evolution:** the DST-ambiguity policy (the one open item below) is now **settled** — ambiguous → `.earliest()`, nonexistent → shift forward (see §4 B1) — and null ordering is **decided** (keep current behavior + ADR, §3). Everything else is decided. Post-v4: external prior-art research (`research/`) and the chrono-API audit added the local-wall-clock calendar frame, day-as-calendar, the gap-verified resolver, and the chrono format/week delegation mandates (spec 'Implementation Decisions').

**Post-v4 decisions (defined in `spec.md`):**
- **D12** — calendar frame = local wall clock: day/week/month/year application round-trips `DateTime<Utc>` → local naive → UTC through the resolver; sub-day units remain exact on the instant.
- **D13** — day is a calendar application unit; identity stays seconds-based (`1d == 24h` as values, may shift differently across DST).
- **D14** — gap-verified `MappedLocalTime` resolver: ambiguous → earliest, true gap → shift forward, tz-data/OS `None` → error.
- **Serde channel** — `Serialize` emits explicit RFC3339 `…Z` via `to_rfc3339_opts(SecondsFormat::Secs, use_z=true)`; human `Display` remains local-naive. The local→UTC rule therefore applies only to naive user-authored input, while serialized instants round-trip exactly.
